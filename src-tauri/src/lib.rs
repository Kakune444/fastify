use futures_util::{StreamExt, stream};
use librespot_core::{
    SpotifyUri, authentication::Credentials, config::SessionConfig, session::Session,
};
use librespot_metadata::{Album, Metadata, Playlist, Track};
use librespot_oauth::OAuthClientBuilder;
use librespot_playback::{
    audio_backend,
    config::{AudioFormat, PlayerConfig},
    mixer::{Mixer, MixerConfig, softmixer::SoftMixer},
    player::{Player, PlayerEvent},
};
use protobuf::Message;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager, State};
use tokio::sync::{Mutex, Semaphore};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppError {
    code: &'static str,
    message: &'static str,
    retry_after_seconds: Option<u64>,
}
type Result<T> = std::result::Result<T, AppError>;
fn error(code: &'static str, message: &'static str) -> AppError {
    AppError {
        code,
        message,
        retry_after_seconds: None,
    }
}
fn network_error(_: impl std::fmt::Display) -> AppError {
    error(
        "NETWORK",
        "Spotify est indisponible. Réessayez dans un instant.",
    )
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImportedAuth {
    access_token: String,
    client_token: Option<String>,
}
struct Native {
    session: Session,
    player: Arc<Player>,
    mixer: SoftMixer,
}
struct Backend {
    client: reqwest::Client,
    native: Mutex<Option<Native>>,
    imported: Mutex<Option<ImportedAuth>>,
    connection: Mutex<()>,
    requests: Semaphore,
    cooldown: Mutex<Option<Instant>>,
    cache: Mutex<HashMap<String, (Instant, Value)>>,
    playback: Mutex<Playback>,
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct TrackView {
    uri: String,
    name: String,
    artist: String,
    album: String,
    image: String,
    duration_ms: u32,
}
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Playback {
    track: Option<TrackView>,
    playing: bool,
    loading: bool,
    position_ms: u32,
    unavailable: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Status {
    connected: bool,
    native_ready: bool,
    username: Option<String>,
}
#[derive(Serialize)]
struct Collection {
    name: String,
    tracks: Vec<TrackView>,
    total: usize,
    offset: usize,
}

impl Backend {
    fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .connect_timeout(Duration::from_secs(5))
                .build()
                .expect("HTTP client"),
            native: Mutex::new(None),
            imported: Mutex::new(None),
            connection: Mutex::new(()),
            requests: Semaphore::new(3),
            cooldown: Mutex::new(None),
            cache: Mutex::new(HashMap::new()),
            playback: Mutex::new(Playback::default()),
        }
    }
    async fn session(&self) -> Result<Session> {
        self.native
            .lock()
            .await
            .as_ref()
            .map(|n| n.session.clone())
            .filter(|s| !s.is_invalid())
            .ok_or_else(|| {
                error(
                    "AUTH_REQUIRED",
                    "Connectez-vous à Spotify pour écouter votre musique.",
                )
            })
    }
    async fn auth(&self) -> Result<ImportedAuth> {
        if let Ok(session) = self.session().await {
            let access =
                tokio::time::timeout(Duration::from_secs(15), session.login5().auth_token())
                    .await
                    .map_err(network_error)?
                    .map_err(network_error)?;
            let client_token =
                tokio::time::timeout(Duration::from_secs(15), session.spclient().client_token())
                    .await
                    .map_err(network_error)?
                    .map_err(network_error)?;
            return Ok(ImportedAuth {
                access_token: access.access_token,
                client_token: Some(client_token),
            });
        }
        self.imported.lock().await.clone().ok_or_else(|| {
            error(
                "AUTH_REQUIRED",
                "Connectez-vous à Spotify pour retrouver votre musique.",
            )
        })
    }
    async fn query(&self, name: &str, variables: Value) -> Result<Value> {
        let key = format!("{name}:{variables}");
        if let Some((time, value)) = self.cache.lock().await.get(&key) {
            if time.elapsed() < Duration::from_secs(45) {
                return Ok(value.clone());
            }
        }
        let _slot = self.requests.acquire().await.map_err(network_error)?;
        if let Some(until) = *self.cooldown.lock().await {
            if until > Instant::now() {
                return Err(AppError {
                    code: "RATE_LIMIT",
                    message: "Spotify demande de patienter avant de réessayer.",
                    retry_after_seconds: Some((until - Instant::now()).as_secs() + 1),
                });
            }
        }
        let auth = self.auth().await?;
        let definitions: Value =
            serde_json::from_str(include_str!("queries.json")).map_err(network_error)?;
        let mut body = definitions
            .get(name)
            .cloned()
            .ok_or_else(|| error("QUERY", "Cette page n'est pas encore disponible."))?;
        body["variables"] = variables;
        let mut request = self
            .client
            .post("https://api-partner.spotify.com/pathfinder/v2/query")
            .bearer_auth(&auth.access_token)
            .header("app-platform", "Win32")
            .json(&body);
        if let Some(token) = auth.client_token {
            request = request.header("client-token", token);
        }
        let response = request.send().await.map_err(network_error)?;
        if response.status().as_u16() == 429 {
            let wait = response
                .headers()
                .get("retry-after")
                .and_then(|h| h.to_str().ok())
                .and_then(|h| h.parse::<u64>().ok())
                .unwrap_or(60)
                .clamp(1, 3600);
            *self.cooldown.lock().await = Some(Instant::now() + Duration::from_secs(wait));
            return Err(AppError {
                code: "RATE_LIMIT",
                message: "Spotify demande de patienter avant de réessayer.",
                retry_after_seconds: Some(wait),
            });
        }
        if matches!(response.status().as_u16(), 401 | 403) {
            self.imported.lock().await.take();
            return Err(error(
                "AUTH_REQUIRED",
                "La session a expiré. Reconnectez-vous à Spotify.",
            ));
        }
        let body: Value = response
            .error_for_status()
            .map_err(network_error)?
            .json()
            .await
            .map_err(network_error)?;
        if body
            .get("errors")
            .and_then(Value::as_array)
            .is_some_and(|e| !e.is_empty())
        {
            return Err(error(
                "API_CHANGED",
                "Cette page Spotify a changé et doit être adaptée.",
            ));
        }
        let mut cache = self.cache.lock().await;
        cache.retain(|_, (time, _)| time.elapsed() < Duration::from_secs(45));
        if cache.len() >= 24 {
            cache.clear();
        }
        cache.insert(key, (Instant::now(), body.clone()));
        Ok(body)
    }
}

#[tauri::command]
async fn get_status(state: State<'_, Arc<Backend>>) -> Result<Status> {
    let native = state.native.lock().await;
    let ready = native.as_ref().is_some_and(|n| !n.session.is_invalid());
    Ok(Status {
        connected: ready || state.imported.lock().await.is_some(),
        native_ready: ready,
        username: native
            .as_ref()
            .filter(|_| ready)
            .map(|n| n.session.username()),
    })
}

#[tauri::command]
async fn connect_spotify(
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
    interactive: bool,
) -> Result<Status> {
    let _connection = state.connection.lock().await;
    if state.session().await.is_ok() {
        return get_status(state.clone()).await;
    }
    let config = SessionConfig::default();
    let vault = keyring::Entry::new("fastify-desktop", "spotify-session")
        .map_err(|_| error("STORAGE", "Le coffre Windows est indisponible."))?;
    let local = std::env::var_os("FASTIFY_SESSION_FILE")
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| {
            serde_json::from_slice::<ImportedAuth>(
                b.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&b),
            )
            .ok()
        });
    let imported_mode = local.is_some();
    let credentials = if let Some(auth) = local {
        *state.imported.lock().await = Some(auth.clone());
        Credentials::with_access_token(auth.access_token)
    } else if let Ok(saved) = vault.get_password() {
        serde_json::from_str(&saved)
            .map_err(|_| error("STORAGE", "La session enregistrée n'est plus valide."))?
    } else if interactive {
        let client_id = config.client_id.clone();
        let token = tauri::async_runtime::spawn_blocking(move || {
            OAuthClientBuilder::new(&client_id, "http://127.0.0.1:8898/login", vec!["streaming"])
                .open_in_browser()
                .with_custom_message("Connexion terminée. Vous pouvez revenir dans Fastify.")
                .build()
                .and_then(|client| client.get_access_token())
        })
        .await
        .map_err(network_error)?
        .map_err(|_| {
            error(
                "AUTH_REQUIRED",
                "La connexion Spotify n'a pas abouti. Réessayez.",
            )
        })?;
        Credentials::with_access_token(token.access_token)
    } else {
        return Err(error("AUTH_REQUIRED", "Connectez-vous à Spotify."));
    };
    let session = Session::new(config, None);
    let connected =
        tokio::time::timeout(Duration::from_secs(35), session.connect(credentials, false)).await;
    if !matches!(connected, Ok(Ok(()))) {
        session.shutdown();
        if imported_mode {
            return get_status(state.clone()).await;
        }
        return Err(error(
            "AUTH_REQUIRED",
            "La connexion native Spotify a échoué. Réessayez de vous connecter.",
        ));
    }
    if !imported_mode {
        let reusable = Credentials { username: Some(session.username()), auth_data: session.auth_data(), auth_type: librespot_protocol::authentication::AuthenticationType::AUTHENTICATION_STORED_SPOTIFY_CREDENTIALS };
        let saved = serde_json::to_string(&reusable).map_err(network_error)?;
        if vault.set_password(&saved).is_err() {
            session.shutdown();
            return Err(error(
                "STORAGE",
                "Impossible d'enregistrer la session dans le coffre Windows.",
            ));
        }
    }
    let mixer = SoftMixer::open(MixerConfig::default()).map_err(network_error)?;
    mixer.set_volume(32768);
    let backend = audio_backend::find(Some("rodio".to_owned()))
        .ok_or_else(|| error("AUDIO", "Aucune sortie audio n'est disponible."))?;
    let player_config = PlayerConfig {
        position_update_interval: Some(Duration::from_secs(1)),
        ..PlayerConfig::default()
    };
    let player = Player::new(
        player_config,
        session.clone(),
        mixer.get_soft_volume(),
        move || backend(None, AudioFormat::default()),
    );
    let mut events = player.get_player_event_channel();
    *state.native.lock().await = Some(Native {
        session,
        player,
        mixer,
    });
    let shared = state.inner().clone();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = events.recv().await {
            let mut snapshot = shared.playback.lock().await;
            match event {
                PlayerEvent::Playing { position_ms, .. } => {
                    snapshot.playing = true;
                    snapshot.loading = false;
                    snapshot.position_ms = position_ms;
                }
                PlayerEvent::Paused { position_ms, .. } => {
                    snapshot.playing = false;
                    snapshot.position_ms = position_ms;
                }
                PlayerEvent::PositionChanged { position_ms, .. }
                | PlayerEvent::Seeked { position_ms, .. } => {
                    snapshot.position_ms = position_ms;
                }
                PlayerEvent::Unavailable { .. } => {
                    snapshot.playing = false;
                    snapshot.loading = false;
                    snapshot.unavailable = true;
                }
                PlayerEvent::EndOfTrack { .. } => {
                    snapshot.playing = false;
                    let _ = app.emit("track-ended", ());
                }
                PlayerEvent::Stopped { .. } => {
                    snapshot.playing = false;
                    snapshot.loading = false;
                }
                _ => continue,
            }
            let _ = app.emit("playback", &*snapshot);
        }
    });
    get_status(state.clone()).await
}

#[tauri::command]
async fn disconnect_spotify(state: State<'_, Arc<Backend>>) -> Result<()> {
    let _connection = state.connection.lock().await;
    if let Some(native) = state.native.lock().await.take() {
        native.player.stop();
        native.session.shutdown();
    }
    state.imported.lock().await.take();
    state.cache.lock().await.clear();
    *state.playback.lock().await = Playback::default();
    if let Ok(entry) = keyring::Entry::new("fastify-desktop", "spotify-session") {
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(_) => {
                return Err(error(
                    "STORAGE",
                    "La session locale n'a pas pu être supprimée.",
                ));
            }
        }
    }
    Ok(())
}

#[tauri::command]
async fn get_home(state: State<'_, Arc<Backend>>, timezone: String) -> Result<Value> {
    if timezone.len() > 80 {
        return Err(error("INPUT", "Fuseau horaire invalide."));
    }
    state.query("home", json!({ "homeEndUserIntegration":"INTEGRATION_DESKTOP", "timeZone":timezone, "sp_t":"", "facet":"", "sectionItemsLimit":12, "includeEpisodeContentRatingsV2":true })).await
}

#[tauri::command]
async fn search_catalog(state: State<'_, Arc<Backend>>, query: String) -> Result<Value> {
    let query = query.trim();
    if query.is_empty() || query.chars().count() > 200 {
        return Err(error(
            "INPUT",
            "Saisissez une recherche de 1 à 200 caractères.",
        ));
    }
    state.query("searchDesktop", json!({ "searchTerm":query, "offset":0, "limit":20, "numberOfTopResults":5, "includeAuthors":true, "includeAudiobooks":true, "includeAlbumPreReleases":false, "includeEpisodeContentRatingsV2":true })).await
}

#[tauri::command]
async fn get_library(state: State<'_, Arc<Backend>>, offset: Option<usize>) -> Result<Value> {
    let session = state.session().await?;
    let offset = offset.unwrap_or(0);
    if offset > 100_000 {
        return Err(error("INPUT", "Page de bibliothèque invalide."));
    }
    let bytes = tokio::time::timeout(
        Duration::from_secs(15),
        session.spclient().get_rootlist(offset, Some(40)),
    )
    .await
    .map_err(network_error)?
    .map_err(network_error)?;
    let root =
        librespot_protocol::playlist4_external::SelectedListContent::parse_from_bytes(&bytes)
            .map_err(network_error)?;
    let mut page = library_page(&root, offset);
    let uris: Vec<_> = page["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|i| i["uri"].as_str())
        .collect();
    if !uris.is_empty() {
        // This operation supplies artwork only; names come from decorated rootlist metadata.
        if let Ok(lookup) = state
            .query("lookupChildEntities", json!({"uris":uris}))
            .await
        {
            if let (Some(items), Some(entities)) = (
                page["items"].as_array_mut(),
                lookup["data"]["lookupEntities"].as_array(),
            ) {
                for item in items {
                    if let Some(entity) =
                        entities.iter().find(|entity| entity["uri"] == item["uri"])
                    {
                        item["images"] = entity["visualIdentityTrait"]["squareCoverImage"]["image"]
                            ["data"]
                            .clone();
                    }
                }
            }
        }
    }
    Ok(page)
}

fn library_page(
    root: &librespot_protocol::playlist4_external::SelectedListContent,
    offset: usize,
) -> Value {
    let contents = root.contents.as_ref();
    let items: Vec<Value> = contents
        .into_iter()
        .flat_map(|c| {
            c.items.iter().enumerate().filter_map(|(index, item)| {
                let SpotifyUri::Playlist { id, .. } = SpotifyUri::from_uri(item.uri()).ok()? else {
                    return None;
                };
                let uri = SpotifyUri::Playlist { id, user: None }.to_uri().ok()?;
                let meta = c.meta_items.get(index);
                let name = meta
                    .and_then(|m| m.attributes.as_ref())
                    .map(|a| a.name())
                    .filter(|n| !n.is_empty())
                    .unwrap_or("Playlist");
                let owner = meta.map(|m| m.owner_username()).unwrap_or("");
                Some(json!({"uri":uri,"name":name,"ownerV2":{"data":{"name":owner}}}))
            })
        })
        .collect();
    let consumed = contents.map_or(0, |c| c.items.len());
    let total = root.length().max(0) as usize;
    let next = offset.saturating_add(consumed);
    json!({"items":items,"total":total,"offset":offset,"nextOffset":if consumed > 0 && next < total {Some(next)} else {None}})
}

fn track_view(track: Track) -> TrackView {
    TrackView {
        uri: track.id.to_uri().unwrap_or_default(),
        name: track.name,
        artist: track
            .artists
            .iter()
            .map(|a| a.name.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        album: track.album.name,
        image: track
            .album
            .covers
            .first()
            .map(|i| format!("https://i.scdn.co/image/{}", i.id))
            .unwrap_or_default(),
        duration_ms: track.duration.max(0) as u32,
    }
}

#[tauri::command]
async fn open_collection(
    state: State<'_, Arc<Backend>>,
    uri: String,
    offset: usize,
) -> Result<Collection> {
    let session = state.session().await?;
    let id = SpotifyUri::from_uri(&uri).map_err(|_| error("INPUT", "Lien Spotify invalide."))?;
    let (name, ids): (String, Vec<SpotifyUri>) = match id {
        SpotifyUri::Album { .. } => {
            let item = tokio::time::timeout(Duration::from_secs(15), Album::get(&session, &id))
                .await
                .map_err(network_error)?
                .map_err(network_error)?;
            (item.name.clone(), item.tracks().cloned().collect())
        }
        SpotifyUri::Playlist { .. } => {
            let item = tokio::time::timeout(Duration::from_secs(15), Playlist::get(&session, &id))
                .await
                .map_err(network_error)?
                .map_err(network_error)?;
            (item.name().to_owned(), item.tracks().cloned().collect())
        }
        _ => {
            return Err(error(
                "UNAVAILABLE",
                "Cette page n'est pas encore disponible dans ce prototype.",
            ));
        }
    };
    let total = ids.len();
    let tracks: Vec<_> = stream::iter(ids.into_iter().skip(offset).take(40))
        .map(|id| {
            let session = session.clone();
            async move {
                tokio::time::timeout(Duration::from_secs(15), Track::get(&session, &id))
                    .await
                    .ok()
                    .and_then(|r| r.ok())
                    .map(track_view)
            }
        })
        .buffered(4)
        .filter_map(|item| async move { item })
        .collect()
        .await;
    Ok(Collection {
        name,
        tracks,
        total,
        offset,
    })
}

#[tauri::command]
async fn play_track(
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
    uri: String,
) -> Result<()> {
    let id = SpotifyUri::from_uri(&uri).map_err(|_| error("INPUT", "Lien Spotify invalide."))?;
    if !matches!(id, SpotifyUri::Track { .. }) {
        return Err(error(
            "UNAVAILABLE",
            "La lecture des podcasts n'est pas encore disponible.",
        ));
    }
    let session = state.session().await?;
    let track = tokio::time::timeout(Duration::from_secs(15), Track::get(&session, &id))
        .await
        .map_err(network_error)?
        .map_err(network_error)?;
    let snapshot = Playback {
        track: Some(track_view(track)),
        loading: true,
        ..Playback::default()
    };
    *state.playback.lock().await = snapshot.clone();
    let _ = app.emit("playback", snapshot);
    let native = state.native.lock().await;
    native
        .as_ref()
        .ok_or_else(|| error("AUTH_REQUIRED", "Reconnectez-vous à Spotify."))?
        .player
        .load(id, true, 0);
    Ok(())
}

#[tauri::command]
async fn player_command(
    state: State<'_, Arc<Backend>>,
    command: String,
    value: Option<u32>,
) -> Result<()> {
    let native = state.native.lock().await;
    let native = native
        .as_ref()
        .ok_or_else(|| error("AUTH_REQUIRED", "Connectez-vous à Spotify."))?;
    match command.as_str() {
        "play" => native.player.play(),
        "pause" => native.player.pause(),
        "seek" => {
            let duration = state
                .playback
                .lock()
                .await
                .track
                .as_ref()
                .map(|t| t.duration_ms)
                .unwrap_or(0);
            native.player.seek(value.unwrap_or(0).min(duration));
        }
        "volume" => native
            .mixer
            .set_volume((value.unwrap_or(50).min(100) as f64 * 655.35) as u16),
        _ => return Err(error("INPUT", "Commande inconnue.")),
    }
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .manage(Arc::new(Backend::new()))
        .invoke_handler(tauri::generate_handler![
            get_status,
            connect_spotify,
            disconnect_spotify,
            get_home,
            search_catalog,
            get_library,
            open_collection,
            play_track,
            player_command
        ])
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::Destroyed) {
                let shared = window.state::<Arc<Backend>>().inner().clone();
                tauri::async_runtime::spawn(async move {
                    if let Some(native) = shared.native.lock().await.take() {
                        native.player.stop();
                        native.session.shutdown();
                    }
                });
            }
        })
        .run(tauri::generate_context!())
        .expect("Fastify could not start");
}

#[cfg(test)]
mod tests {
    use super::*;
    use librespot_protocol::playlist4_external::{
        Item, ListAttributes, ListItems, MetaItem, SelectedListContent,
    };

    #[test]
    fn rootlist_keeps_names_aligned_when_folders_are_filtered() {
        let mut root = SelectedListContent::new();
        root.set_length(8);
        let mut contents = ListItems::new();
        contents.set_pos(0);
        contents.set_truncated(true);
        for (uri, name) in [
            ("spotify:start-group:folder", "Dossier"),
            (
                "spotify:user:owner:playlist:37i9dQZF1DXcBWIGoYBM5M",
                "Ma playlist",
            ),
        ] {
            let mut item = Item::new();
            item.set_uri(uri.into());
            contents.items.push(item);
            let mut meta = MetaItem::new();
            let mut attributes = ListAttributes::new();
            attributes.set_name(name.into());
            meta.attributes = Some(attributes).into();
            contents.meta_items.push(meta);
        }
        root.contents = Some(contents).into();
        let page = library_page(&root, 0);
        assert_eq!(page["items"].as_array().unwrap().len(), 1);
        assert_eq!(page["items"][0]["name"], "Ma playlist");
        assert_eq!(
            page["items"][0]["uri"],
            "spotify:playlist:37i9dQZF1DXcBWIGoYBM5M"
        );
        assert_eq!(page["nextOffset"], 2);
    }

    #[test]
    fn empty_rootlist_does_not_repeat_the_same_page() {
        let page = library_page(&SelectedListContent::new(), 40);
        assert!(page["nextOffset"].is_null());
        assert!(page["items"].as_array().unwrap().is_empty());
    }
}
