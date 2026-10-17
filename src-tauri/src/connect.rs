//! Account-scoped Connect state. Only the public view crosses IPC; transfer data stays here.
use librespot_connect::{
    ConnectConfig, LoadContextOptions, LoadRequest, LoadRequestOptions, Options, PlayingTrack,
    Spirc,
};
use librespot_core::{
    SpotifyUri, authentication::Credentials, config::DeviceType, session::Session,
};
use librespot_playback::{mixer::Mixer, player::Player};
use librespot_protocol::{connect::Cluster, player::ProvidedTrack};
use protobuf::Message;
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{Mutex, watch};

#[derive(Debug)]
pub enum ConnectError {
    Input,
    NotReady,
    Missing,
    Restricted,
    Conflict,
    RateLimited(u64),
    Auth,
    Network,
    Uncertain,
    Protocol,
}
pub type Result<T> = std::result::Result<T, ConnectError>;

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectTrack {
    pub uri: String,
    pub name: String,
    pub artist: String,
    pub album: String,
    pub image: String,
    pub duration_ms: u32,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueTrack {
    pub key: String,
    pub queued: bool,
    #[serde(flatten)]
    pub track: ConnectTrack,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceView {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub local: bool,
    pub active: bool,
    pub available: bool,
    pub volume: Option<u32>,
}
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Restrictions {
    pub pause: bool,
    pub resume: bool,
    pub seek: bool,
    pub next: bool,
    pub previous: bool,
    pub shuffle: bool,
    pub repeat: bool,
    pub add: bool,
    pub edit_queue: bool,
}
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectView {
    pub available: bool,
    pub ready: bool,
    pub local_device_id: String,
    pub active_device_id: String,
    pub devices: Vec<DeviceView>,
    pub track: Option<ConnectTrack>,
    pub playing: bool,
    pub loading: bool,
    pub position_ms: u32,
    pub shuffle: bool,
    pub repeat: String,
    pub revision: String,
    pub context_name: String,
    pub queue: Vec<QueueTrack>,
    pub restrictions: Restrictions,
}

fn transfer_ready(current: &ConnectView, previous: &ConnectView, destination: &str) -> bool {
    if current.active_device_id != destination {
        return false;
    }
    // Ownership arrives before librespot finishes resolving the transferred context.
    // A paused receiver also sets the buffering bit after it has finished loading.
    if let Some(track) = &previous.track {
        if !current
            .track
            .as_ref()
            .is_some_and(|t| t.uri == track.uri && t.duration_ms > 0)
        {
            return false;
        }
    }
    if destination != current.local_device_id {
        return true;
    }
    current.queue.len() >= previous.queue.len().min(2)
}

pub struct ConnectModel {
    local: String,
    pub cluster: Option<Cluster>,
    ready: bool,
    last_timestamp: i64,
}
impl ConnectModel {
    pub fn new(local: String) -> Self {
        Self {
            local,
            cluster: None,
            ready: false,
            last_timestamp: 0,
        }
    }
    pub fn apply(&mut self, cluster: Cluster) -> bool {
        if cluster.changed_timestamp_ms < self.last_timestamp {
            return false;
        }
        self.last_timestamp = cluster.changed_timestamp_ms;
        self.ready = true;
        self.cluster = Some(cluster);
        true
    }
    fn revision(&self) -> String {
        let mut hash = DefaultHasher::new();
        if let Some(c) = &self.cluster {
            c.active_device_id.hash(&mut hash);
            c.player_state.playback_id.hash(&mut hash);
            c.player_state.queue_revision.hash(&mut hash);
            c.player_state.track.uri.hash(&mut hash);
            c.player_state.track.uid.hash(&mut hash);
            for t in &c.player_state.next_tracks {
                t.write_to_bytes().unwrap_or_default().hash(&mut hash);
            }
        }
        format!("{:016x}", hash.finish())
    }
    pub fn view(&self) -> ConnectView {
        let mut view = ConnectView {
            available: true,
            ready: self.ready,
            local_device_id: self.local.clone(),
            repeat: "off".into(),
            revision: self.revision(),
            ..ConnectView::default()
        };
        let Some(c) = &self.cluster else {
            view.devices.push(DeviceView {
                id: self.local.clone(),
                name: "Spotisloth — Cet ordinateur".into(),
                kind: "computer".into(),
                local: true,
                active: false,
                available: self.ready,
                volume: Some(50),
            });
            return view;
        };
        view.active_device_id = c.active_device_id.clone();
        view.devices = c
            .device
            .iter()
            .filter(|(_, d)| !d.capabilities.hidden)
            .map(|(id, d)| DeviceView {
                id: id.clone(),
                name: if id == &self.local {
                    "Spotisloth — Cet ordinateur".into()
                } else {
                    d.name.clone()
                },
                kind: match d.device_type.value() {
                    1 => "computer",
                    2 => "tablet",
                    3 => "phone",
                    5 | 11 => "tv",
                    _ => "speaker",
                }
                .into(),
                local: id == &self.local,
                active: id == &c.active_device_id,
                available: d.can_play
                    && !d.is_offline
                    && !d.capabilities.connect_disabled
                    && d.disallow_transfer_reasons.is_empty(),
                volume: (!d.capabilities.disable_volume)
                    .then_some((d.volume.min(65535) * 100 + 32767) / 65535),
            })
            .collect();
        if !view.devices.iter().any(|d| d.local) {
            view.devices.push(DeviceView {
                id: self.local.clone(),
                name: "Spotisloth — Cet ordinateur".into(),
                kind: "computer".into(),
                local: true,
                active: false,
                available: self.ready,
                volume: Some(50),
            });
        }
        view.devices.sort_by(|a, b| {
            b.active
                .cmp(&a.active)
                .then(b.local.cmp(&a.local))
                .then(a.name.cmp(&b.name))
                .then(a.id.cmp(&b.id))
        });
        let p = &c.player_state;
        if c.active_device_id.is_empty() {
            return view;
        }
        view.track = p
            .track
            .as_ref()
            .filter(|t| !t.uri.is_empty())
            .map(|t| track_view(t, p.duration.max(0).min(u32::MAX as i64) as u32));
        view.playing = p.is_playing && !p.is_paused && !p.is_buffering;
        // Connect encodes ordinary pauses with all three status flags set.
        // Paused takes precedence so that the resume control stays available.
        view.loading = p.is_buffering && !p.is_paused;
        let elapsed = if view.playing {
            now_ms().saturating_sub(p.timestamp).max(0)
        } else {
            0
        };
        view.position_ms = (p.position_as_of_timestamp.max(0).saturating_add(elapsed))
            .min(p.duration.max(0))
            .min(u32::MAX as i64) as u32;
        view.shuffle = p.options.shuffling_context;
        view.repeat = if p.options.repeating_track {
            "one"
        } else if p.options.repeating_context {
            "all"
        } else {
            "off"
        }
        .into();
        view.context_name = p
            .context_metadata
            .get("context_description")
            .cloned()
            .unwrap_or_else(|| "Spotify Connect".into());
        view.queue = p
            .next_tracks
            .iter()
            .enumerate()
            .take_while(|(_, t)| t.uri != "spotify:delimiter")
            .filter(|(_, t)| t.metadata.get("hidden").is_none_or(|v| v != "true"))
            .map(|(i, t)| QueueTrack {
                key: format!("{i}:{}", t.uid),
                queued: t.provider == "queue"
                    || t.metadata.get("is_queued").is_some_and(|v| v == "true"),
                track: track_view(t, 0),
            })
            .collect();
        let r = &p.restrictions;
        view.restrictions = Restrictions {
            pause: !r.disallow_pausing_reasons.is_empty(),
            resume: !r.disallow_resuming_reasons.is_empty(),
            seek: !r.disallow_seeking_reasons.is_empty(),
            next: !r.disallow_skipping_next_reasons.is_empty(),
            previous: !r.disallow_skipping_prev_reasons.is_empty(),
            shuffle: !r.disallow_toggling_shuffle_reasons.is_empty(),
            repeat: !r.disallow_toggling_repeat_context_reasons.is_empty()
                || !r.disallow_toggling_repeat_track_reasons.is_empty(),
            add: !r.disallow_add_to_queue_reasons.is_empty(),
            edit_queue: !r.disallow_set_queue_reasons.is_empty()
                || !r.disallow_reordering_in_next_tracks_reasons.is_empty()
                || !r.disallow_removing_from_next_tracks_reasons.is_empty(),
        };
        view
    }
    fn queue_index(&self, revision: &str, key: &str) -> Result<usize> {
        if revision != self.revision() {
            return Err(ConnectError::Conflict);
        }
        self.cluster
            .as_ref()
            .ok_or(ConnectError::NotReady)?
            .player_state
            .next_tracks
            .iter()
            .enumerate()
            .find(|(i, t)| format!("{i}:{}", t.uid) == key)
            .map(|(i, _)| i)
            .ok_or(ConnectError::Missing)
    }
}
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}
fn track_view(t: &ProvidedTrack, duration: u32) -> ConnectTrack {
    let get = |key: &str| t.metadata.get(key).cloned().unwrap_or_default();
    let image = get("image_url");
    ConnectTrack {
        uri: t.uri.clone(),
        name: t
            .metadata
            .get("title")
            .cloned()
            .unwrap_or_else(|| "Chargement du titre…".into()),
        artist: get("artist_name"),
        album: get("album_title"),
        image: image
            .strip_prefix("spotify:image:")
            .map(|id| format!("https://i.scdn.co/image/{id}"))
            .unwrap_or(image),
        duration_ms: if duration > 0 {
            duration
        } else {
            get("duration").parse().unwrap_or(0)
        },
    }
}

#[derive(Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ConnectAction {
    Transfer {
        #[serde(rename = "deviceId")]
        device_id: String,
    },
    Play {
        uri: String,
        context: Option<String>,
        tracks: Option<Vec<String>>,
        index: Option<u32>,
        shuffle: bool,
        repeat: String,
    },
    Pause,
    Resume,
    Next,
    Previous,
    Seek {
        value: u32,
    },
    Volume {
        value: u32,
    },
    Shuffle {
        value: bool,
    },
    Repeat {
        value: String,
    },
    Add {
        uri: String,
    },
    Jump {
        key: String,
        revision: String,
    },
    Remove {
        key: String,
        revision: String,
    },
    Move {
        key: String,
        revision: String,
        to: usize,
    },
}
pub struct ConnectRuntime {
    pub spirc: Spirc,
    pub model: Arc<Mutex<ConnectModel>>,
    sender: watch::Sender<ConnectView>,
    session: Session,
    client: Client,
    commands: Mutex<()>,
    cooldown: Mutex<Option<Instant>>,
    message_id: AtomicU32,
    observer: tokio::task::JoinHandle<()>,
    engine: tokio::task::JoinHandle<()>,
}
impl Drop for ConnectRuntime {
    fn drop(&mut self) {
        let _ = self.spirc.shutdown();
        self.observer.abort();
        self.engine.abort();
    }
}
impl ConnectRuntime {
    pub async fn start(
        session: Session,
        credentials: Credentials,
        player: Arc<Player>,
        mixer: Arc<dyn Mixer>,
        client: Client,
    ) -> Result<Self> {
        let (cluster_sender, mut clusters) = watch::channel(Cluster::new());
        let (spirc, task) = tokio::time::timeout(
            Duration::from_secs(40),
            Spirc::new_with_state(
                ConnectConfig {
                    name: "Spotisloth".into(),
                    device_type: DeviceType::Computer,
                    ..ConnectConfig::default()
                },
                session.clone(),
                credentials,
                player,
                mixer,
                Some(cluster_sender),
            ),
        )
        .await
        .map_err(|_| ConnectError::Network)?
        .map_err(|_| ConnectError::Network)?;
        let mut initial = ConnectModel::new(session.device_id().into());
        initial.ready = false;
        let model = Arc::new(Mutex::new(initial));
        let (sender, _) = watch::channel(model.lock().await.view());
        let shared = model.clone();
        let updates = sender.clone();
        let observed_session = session.clone();
        let observer = tokio::spawn(async move {
            let mut heartbeat = tokio::time::interval(Duration::from_secs(5));
            loop {
                tokio::select! {
                    update = clusters.changed() => {
                        if update.is_err() {break;}
                        let cluster=clusters.borrow_and_update().clone();
                        let mut m=shared.lock().await;
                        if m.apply(cluster) {updates.send_replace(m.view());}
                    }
                    _ = heartbeat.tick() => if observed_session.is_invalid() { break; }
                }
            }
            let mut m = shared.lock().await;
            m.ready = false;
            updates.send_replace(m.view());
        });
        let engine = tokio::spawn(task);
        Ok(Self {
            spirc,
            model,
            sender,
            session,
            client,
            commands: Mutex::new(()),
            cooldown: Mutex::new(None),
            message_id: AtomicU32::new(1),
            observer,
            engine,
        })
    }
    pub fn subscribe(&self) -> watch::Receiver<ConnectView> {
        self.sender.subscribe()
    }
    pub async fn view(&self) -> ConnectView {
        self.model.lock().await.view()
    }
    /// Local development counters only. Never exposed through the application's IPC.
    pub async fn queue_diagnostics(&self) -> Value {
        let model = self.model.lock().await;
        let Some(cluster) = &model.cluster else {
            return json!({"ready":false});
        };
        let tracks = &cluster.player_state.next_tracks;
        json!({
            "queue_count":tracks.len(),
            "non_track_entries":tracks.iter().filter(|t| validate_track(&t.uri).is_err()).count(),
            "samples":tracks.iter().take(4).map(|t| {
                let mut keys=t.metadata.keys().cloned().collect::<Vec<_>>();
                keys.sort();
                json!({"uri_length":t.uri.len(),"valid_track":validate_track(&t.uri).is_ok(),"has_uid":!t.uid.is_empty(),"metadata_keys":keys,"removed_fields":t.removed,"unknown_fields":t.special_fields.unknown_fields().iter().map(|(number,_)|number).collect::<Vec<_>>()})
            }).collect::<Vec<_>>()
        })
    }
    pub fn shutdown(&self) {
        let _ = self.spirc.shutdown();
    }
    async fn send(&self, method: Method, path: &str, body: Option<Value>) -> Result<()> {
        let auth =
            tokio::time::timeout(Duration::from_secs(15), self.session.login5().auth_token())
                .await
                .map_err(|_| ConnectError::Network)?
                .map_err(|_| ConnectError::Auth)?;
        let base = self
            .session
            .spclient()
            .base_url()
            .await
            .map_err(|_| ConnectError::Network)?;
        let token = self
            .session
            .spclient()
            .client_token()
            .await
            .map_err(|_| ConnectError::Network)?;
        let mut request = self
            .client
            .request(method.clone(), format!("{base}{path}"))
            .bearer_auth(auth.access_token)
            .header("client-token", token);
        if let Some(body) = body {
            request = request.json(&body);
        }
        // Writes are submitted once. A lost acknowledgement must not duplicate next/add/transfer.
        let response = request.send().await.map_err(|_| ConnectError::Uncertain)?;
        if std::env::var("SPOTISLOTH_CONNECT_TRACE").as_deref() == Ok("1") {
            let operation = if path.contains("/volume/") {
                "volume"
            } else if path.contains("/transfer/") {
                "transfer"
            } else {
                "player"
            };
            eprintln!(
                "CONNECT_HTTP: operation={operation} method={method} status={}",
                response.status().as_u16()
            );
        }
        match response.status().as_u16() {
            200..=299 => Ok(()),
            401 => Err(ConnectError::Auth),
            403 => Err(ConnectError::Restricted),
            404 => Err(ConnectError::Missing),
            409 => Err(ConnectError::Conflict),
            429 => {
                let wait = response
                    .headers()
                    .get("retry-after")
                    .and_then(|h| h.to_str().ok())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(30)
                    .clamp(1, 3600);
                *self.cooldown.lock().await = Some(Instant::now() + Duration::from_secs(wait));
                Err(ConnectError::RateLimited(wait))
            }
            _ => Err(ConnectError::Uncertain),
        }
    }
    async fn remote(&self, target: &str, mut command: Value) -> Result<()> {
        command["logging_params"] = json!({"command_id":uuid::Uuid::new_v4().simple().to_string()});
        self.send(Method::POST,&format!("/connect-state/v1/player/command/from/{}/to/{target}",self.session.device_id()), Some(json!({ "command":command, "message_id":self.message_id.fetch_add(1,Ordering::Relaxed), "sent_by_device_id":self.session.device_id() }))).await
    }
    pub async fn execute(
        &self,
        action: ConnectAction,
        expected_device: Option<&str>,
    ) -> Result<()> {
        let _guard = self.commands.lock().await;
        if let Some(until) = *self.cooldown.lock().await {
            if until > Instant::now() {
                return Err(ConnectError::RateLimited(
                    (until - Instant::now()).as_secs() + 1,
                ));
            }
        }
        let m = self.model.lock().await;
        let view = m.view();
        if !view.ready || self.session.is_invalid() {
            return Err(ConnectError::NotReady);
        }
        let local = self.session.device_id();
        let target = if view.active_device_id.is_empty() {
            local
        } else {
            &view.active_device_id
        };
        if expected_device.is_some_and(|id| id != target) {
            return Err(ConnectError::Conflict);
        }
        let native = target == local;
        match action {
            ConnectAction::Transfer { device_id } => {
                if !view
                    .devices
                    .iter()
                    .any(|d| d.id == device_id && d.available)
                {
                    return Err(ConnectError::Missing);
                }
                if device_id == view.active_device_id {
                    return Ok(());
                }
                drop(m);
                let started = Instant::now();
                self.send(
                    Method::POST,
                    &format!("/connect-state/v1/connect/transfer/from/{local}/to/{device_id}"),
                    Some(json!({"command_id":uuid::Uuid::new_v4().simple().to_string(),"transfer_options":{}})),
                )
                .await?;
                let mut updates = self.subscribe();
                tokio::time::timeout(Duration::from_secs(12), async {
                    loop {
                        if transfer_ready(&self.view().await, &view, &device_id) {
                            return Ok(());
                        }
                        updates
                            .changed()
                            .await
                            .map_err(|_| ConnectError::Uncertain)?;
                    }
                })
                .await
                .map_err(|_| ConnectError::Uncertain)??;
                // Some receivers restart the same track at zero or resume a paused
                // handoff. Restore explicit controls only after ownership and track
                // identity have been confirmed. Never retry the transfer itself.
                let received = self.view().await;
                let receiving_here = device_id == local;
                if !view.playing && (received.playing || received.loading) {
                    if receiving_here {
                        self.spirc.pause().map_err(|_| ConnectError::Network)?;
                    } else {
                        self.remote(&device_id, json!({"endpoint":"pause"})).await?;
                    }
                } else if view.playing && !received.playing && !received.loading {
                    if receiving_here {
                        self.spirc.play().map_err(|_| ConnectError::Network)?;
                    } else {
                        self.remote(&device_id, json!({"endpoint":"resume"}))
                            .await?;
                    }
                }
                let Some(original_track) = &view.track else {
                    return Ok(());
                };
                let expected_position = || {
                    view.position_ms
                        .saturating_add(if view.playing {
                            started.elapsed().as_millis().min(u32::MAX as u128) as u32
                        } else {
                            0
                        })
                        .min(original_track.duration_ms)
                };
                let current = self.view().await;
                if current.active_device_id != device_id
                    || !current
                        .track
                        .as_ref()
                        .is_some_and(|t| t.uri == original_track.uri)
                {
                    return Err(ConnectError::Conflict);
                }
                let position = expected_position();
                if current.position_ms.abs_diff(position) > 1000 {
                    if receiving_here {
                        self.spirc
                            .set_position_ms(position)
                            .map_err(|_| ConnectError::Network)?;
                    } else {
                        self.remote(
                            &device_id,
                            json!({"endpoint":"seek_to","value":position,"position":position}),
                        )
                        .await?;
                    }
                }
                return tokio::time::timeout(Duration::from_secs(12), async {
                    loop {
                        let current = self.view().await;
                        if current.active_device_id != device_id
                            || !current
                                .track
                                .as_ref()
                                .is_some_and(|t| t.uri == original_track.uri)
                        {
                            return Err(ConnectError::Conflict);
                        }
                        if current.playing == view.playing
                            && !current.loading
                            && current.position_ms.abs_diff(expected_position()) <= 2000
                        {
                            return Ok(());
                        }
                        updates
                            .changed()
                            .await
                            .map_err(|_| ConnectError::Uncertain)?;
                    }
                })
                .await
                .map_err(|_| ConnectError::Uncertain)?;
            }
            ConnectAction::Play {
                uri,
                context,
                tracks,
                index,
                shuffle,
                repeat,
            } => {
                validate_track(&uri)?;
                let repeat = repeat_options(&repeat)?;
                let context = context.map(|c| {
                    if c == "spotify:collection:tracks" {
                        format!("spotify:user:{}:collection", self.session.username())
                    } else {
                        c
                    }
                });
                if let Some(c) = &context {
                    if !c.starts_with("spotify:album:")
                        && !c.starts_with("spotify:playlist:")
                        && c != &format!("spotify:user:{}:collection", self.session.username())
                    {
                        return Err(ConnectError::Input);
                    }
                }
                let tracks = tracks.unwrap_or_else(|| vec![uri.clone()]);
                if tracks.is_empty()
                    || tracks.len() > 1000
                    || tracks.iter().any(|t| validate_track(t).is_err())
                    || index.is_some_and(|i| i > 100_000)
                {
                    return Err(ConnectError::Input);
                }
                let playing_track = if let Some(i) = index {
                    PlayingTrack::Index(i)
                } else {
                    PlayingTrack::Uri(uri.clone())
                };
                let options = LoadRequestOptions {
                    start_playing: true,
                    playing_track: Some(playing_track),
                    context_options: Some(LoadContextOptions::Options(Options {
                        shuffle,
                        repeat: repeat.0,
                        repeat_track: repeat.1,
                    })),
                    ..LoadRequestOptions::default()
                };
                drop(m);
                if native {
                    self.spirc.activate().map_err(|_| ConnectError::Network)?;
                    return self
                        .spirc
                        .load(if let Some(c) = context {
                            LoadRequest::from_context_uri(c, options)
                        } else {
                            LoadRequest::from_tracks(tracks, options)
                        })
                        .map_err(|_| ConnectError::Network);
                }
                let context = context.map(|uri|json!({"url":format!("context://{uri}"),"uri":uri,"metadata":{}})).unwrap_or_else(||json!({"uri":"","url":"","metadata":{},"pages":[{"tracks":tracks.iter().map(|uri|json!({"uri":uri})).collect::<Vec<_>>()}]}));
                let skip = index
                    .map(|i| json!({"track_index":i}))
                    .unwrap_or_else(|| json!({"track_uri":uri}));
                return self.remote(target,json!({"endpoint":"play","context":context,"play_origin":{"feature_identifier":"spotisloth"},"options":{"skip_to":skip,"player_options_override":{"shuffling_context":shuffle,"repeating_context":repeat.0,"repeating_track":repeat.1}}})).await;
            }
            ConnectAction::Pause => {
                if view.restrictions.pause {
                    return Err(ConnectError::Restricted);
                }
                drop(m);
                if native {
                    return self.spirc.pause().map_err(|_| ConnectError::Network);
                }
                self.remote(target, json!({"endpoint":"pause"})).await
            }
            ConnectAction::Resume => {
                if view.restrictions.resume {
                    return Err(ConnectError::Restricted);
                }
                drop(m);
                if native {
                    return self.spirc.play().map_err(|_| ConnectError::Network);
                }
                self.remote(target, json!({"endpoint":"resume"})).await
            }
            ConnectAction::Next | ConnectAction::Previous => {
                let next = matches!(action, ConnectAction::Next);
                if if next {
                    view.restrictions.next
                } else {
                    view.restrictions.previous
                } {
                    return Err(ConnectError::Restricted);
                }
                drop(m);
                if native {
                    return if next {
                        self.spirc.next()
                    } else {
                        self.spirc.prev()
                    }
                    .map_err(|_| ConnectError::Network);
                }
                self.remote(
                    target,
                    json!({"endpoint":if next {"skip_next"} else {"skip_prev"}}),
                )
                .await
            }
            ConnectAction::Seek { value } => {
                if view.restrictions.seek {
                    return Err(ConnectError::Restricted);
                }
                let value = value.min(view.track.as_ref().map_or(0, |t| t.duration_ms));
                drop(m);
                if native {
                    return self
                        .spirc
                        .set_position_ms(value)
                        .map_err(|_| ConnectError::Network);
                }
                self.remote(
                    target,
                    json!({"endpoint":"seek_to","value":value,"position":value}),
                )
                .await
            }
            ConnectAction::Volume { value } => {
                if value > 100 {
                    return Err(ConnectError::Input);
                }
                if view
                    .devices
                    .iter()
                    .find(|d| d.active)
                    .is_some_and(|d| d.volume.is_none())
                {
                    return Err(ConnectError::Restricted);
                }
                let volume = (value * 65535 / 100) as u16;
                drop(m);
                if native {
                    return self
                        .spirc
                        .set_volume(volume)
                        .map_err(|_| ConnectError::Network);
                }
                self.send(
                    Method::PUT,
                    &format!("/connect-state/v1/connect/volume/from/{local}/to/{target}"),
                    Some(json!({"volume":volume})),
                )
                .await
            }
            ConnectAction::Shuffle { value } => {
                if view.restrictions.shuffle {
                    return Err(ConnectError::Restricted);
                }
                drop(m);
                if native {
                    return self.spirc.shuffle(value).map_err(|_| ConnectError::Network);
                }
                self.remote(
                    target,
                    json!({"endpoint":"set_shuffling_context","value":value}),
                )
                .await
            }
            ConnectAction::Repeat { value } => {
                let (context, track) = repeat_options(&value)?;
                if view.restrictions.repeat {
                    return Err(ConnectError::Restricted);
                }
                drop(m);
                if native {
                    self.spirc
                        .repeat(context)
                        .map_err(|_| ConnectError::Network)?;
                    return self
                        .spirc
                        .repeat_track(track)
                        .map_err(|_| ConnectError::Network);
                }
                self.remote(target,json!({"endpoint":"set_options","repeating_context":context,"repeating_track":track})).await
            }
            ConnectAction::Add { uri } => {
                validate_track(&uri)?;
                if view.restrictions.add {
                    return Err(ConnectError::Restricted);
                }
                drop(m);
                self.remote(target,json!({"endpoint":"add_to_queue","track":{"uri":uri,"provider":"queue","metadata":{"is_queued":"true"}}})).await
            }
            ConnectAction::Jump {
                ref key,
                ref revision,
            }
            | ConnectAction::Remove {
                ref key,
                ref revision,
            }
            | ConnectAction::Move {
                ref key,
                ref revision,
                ..
            } => {
                let index = m.queue_index(revision, key)?;
                let p = &m
                    .cluster
                    .as_ref()
                    .ok_or(ConnectError::NotReady)?
                    .player_state;
                let mut tracks = p.next_tracks.clone();
                let encode = |t: &ProvidedTrack| -> Result<Value> {
                    let json = protobuf_json_mapping::print_to_string(t)
                        .map_err(|_| ConnectError::Protocol)?;
                    serde_json::from_str(&json).map_err(|_| ConnectError::Protocol)
                };
                let body = match action {
                    ConnectAction::Jump { .. } => {
                        if view.restrictions.next {
                            return Err(ConnectError::Restricted);
                        }
                        json!({"endpoint":"skip_next","track":encode(&tracks[index])?})
                    }
                    ConnectAction::Remove { .. } => {
                        if view.restrictions.edit_queue {
                            return Err(ConnectError::Restricted);
                        }
                        tracks.remove(index);
                        json!({"endpoint":"set_queue","next_tracks":tracks.iter().map(encode).collect::<Result<Vec<_>>>()?,"prev_tracks":p.prev_tracks.iter().map(encode).collect::<Result<Vec<_>>>()?,"queue_revision":p.queue_revision})
                    }
                    ConnectAction::Move { to, .. } => {
                        if view.restrictions.edit_queue {
                            return Err(ConnectError::Restricted);
                        }
                        let destination = view.queue.get(to).ok_or(ConnectError::Input)?;
                        let to = m.queue_index(&view.revision, &destination.key)?;
                        let t = tracks.remove(index);
                        tracks.insert(to, t);
                        json!({"endpoint":"set_queue","next_tracks":tracks.iter().map(encode).collect::<Result<Vec<_>>>()?,"prev_tracks":p.prev_tracks.iter().map(encode).collect::<Result<Vec<_>>>()?,"queue_revision":p.queue_revision})
                    }
                    _ => unreachable!(),
                };
                drop(m);
                self.remote(target, body).await
            }
        }
    }
}
fn validate_track(uri: &str) -> Result<()> {
    match SpotifyUri::from_uri(uri) {
        Ok(SpotifyUri::Track { .. }) => Ok(()),
        _ => Err(ConnectError::Input),
    }
}
fn repeat_options(value: &str) -> Result<(bool, bool)> {
    match value {
        "off" => Ok((false, false)),
        "all" => Ok((true, false)),
        "one" => Ok((false, true)),
        _ => Err(ConnectError::Input),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use librespot_protocol::{connect::DeviceInfo, player::PlayerState};
    use protobuf::MessageField;
    fn cluster(timestamp: i64) -> Cluster {
        let mut c = Cluster::new();
        c.changed_timestamp_ms = timestamp;
        c.active_device_id = "phone".into();
        let mut p = PlayerState::new();
        p.playback_id = "playback".into();
        p.queue_revision = "r1".into();
        for uid in ["first", "second"] {
            let mut t = ProvidedTrack::new();
            t.uri = "spotify:track:4uLU6hMCjMI75M1A2tKUQC".into();
            t.uid = uid.into();
            p.next_tracks.push(t);
        }
        c.player_state = MessageField::some(p);
        c
    }
    #[test]
    fn stale_cluster_cannot_restore_a_previous_device() {
        let mut m = ConnectModel::new("local".into());
        assert!(m.apply(cluster(20)));
        let mut old = cluster(19);
        old.active_device_id = "old".into();
        assert!(!m.apply(old));
        assert_eq!(m.view().active_device_id, "phone");
    }
    #[test]
    fn queue_occurrences_are_distinct_and_stale_edits_are_refused() {
        let mut m = ConnectModel::new("local".into());
        m.apply(cluster(1));
        let view = m.view();
        assert_ne!(view.queue[0].key, view.queue[1].key);
        assert_eq!(
            m.queue_index(&view.revision, &view.queue[1].key).unwrap(),
            1
        );
        let mut changed = cluster(2);
        changed
            .player_state
            .as_mut()
            .unwrap()
            .next_tracks
            .swap(0, 1);
        m.apply(changed);
        assert!(matches!(
            m.queue_index(&view.revision, &view.queue[1].key),
            Err(ConnectError::Conflict)
        ));
    }
    #[test]
    fn volume_updates_do_not_invalidate_queue_edits() {
        let mut m = ConnectModel::new("local".into());
        m.apply(cluster(1));
        let rev = m.view().revision;
        let mut updated = cluster(2);
        updated.device.insert("phone".into(), DeviceInfo::new());
        m.apply(updated);
        assert_eq!(rev, m.view().revision);
    }
    #[test]
    fn public_view_excludes_transfer_payload_and_device_addresses() {
        let mut m = ConnectModel::new("local".into());
        let mut c = cluster(1);
        c.transfer_data = b"PRIVATE_TRANSFER".to_vec();
        let mut d = DeviceInfo::new();
        d.name = "Phone".into();
        d.public_ip = "PRIVATE_ADDRESS".into();
        c.device.insert("phone".into(), d);
        m.apply(c);
        let text = serde_json::to_string(&m.view()).unwrap();
        assert!(!text.contains("PRIVATE_"));
        assert!(!text.contains("transferData"));
    }
    #[test]
    fn repeat_modes_and_content_are_validated() {
        assert_eq!(repeat_options("one").unwrap(), (false, true));
        assert!(repeat_options("invalid").is_err());
        assert!(validate_track("spotify:album:4uLU6hMCjMI75M1A2tKUQC").is_err());
    }
    #[test]
    fn positions_are_clamped_and_paused_time_does_not_advance() {
        let mut m = ConnectModel::new("local".into());
        let mut c = cluster(1);
        let p = c.player_state.as_mut().unwrap();
        p.duration = 1000;
        p.position_as_of_timestamp = 800;
        p.timestamp = 1;
        p.is_playing = true;
        p.is_paused = true;
        m.apply(c);
        assert_eq!(m.view().position_ms, 800);
        m.cluster
            .as_mut()
            .unwrap()
            .player_state
            .as_mut()
            .unwrap()
            .is_paused = false;
        assert_eq!(m.view().position_ms, 1000);
    }

    #[test]
    fn a_paused_connect_state_with_the_buffering_bit_keeps_resume_available() {
        let mut m = ConnectModel::new("local".into());
        let mut c = cluster(1);
        let p = c.player_state.as_mut().unwrap();
        p.is_playing = true;
        p.is_paused = true;
        p.is_buffering = true;
        m.apply(c);
        let view = m.view();
        assert!(!view.playing);
        assert!(!view.loading);
        assert!(!view.restrictions.resume);
    }

    #[test]
    fn hidden_tracks_keep_the_original_queue_positions_for_edits() {
        let mut m = ConnectModel::new("local".into());
        let mut c = cluster(1);
        let p = c.player_state.as_mut().unwrap();
        let mut delimiter = ProvidedTrack::new();
        delimiter.uri = p.next_tracks[0].uri.clone();
        delimiter.uid = "hidden".into();
        delimiter.metadata.insert("hidden".into(), "true".into());
        p.next_tracks.insert(1, delimiter);
        p.next_tracks[0].provider = "queue".into();
        m.apply(c);
        let view = m.view();
        assert_eq!(view.queue.len(), 2);
        assert!(view.queue[0].queued);
        assert_eq!(
            m.queue_index(&view.revision, &view.queue[1].key).unwrap(),
            2
        );
        assert_eq!(
            m.cluster.as_ref().unwrap().player_state.next_tracks.len(),
            3
        );
    }

    #[test]
    fn future_repeat_iterations_are_kept_by_the_engine_and_not_duplicated_in_the_visible_queue() {
        let mut m = ConnectModel::new("local".into());
        let mut c = cluster(1);
        let p = c.player_state.as_mut().unwrap();
        let mut delimiter = ProvidedTrack::new();
        delimiter.uri = "spotify:delimiter".into();
        delimiter.uid = "delimiter0".into();
        p.next_tracks.insert(1, delimiter);
        p.options = MessageField::some(librespot_protocol::player::ContextPlayerOptions {
            repeating_context: true,
            ..Default::default()
        });
        m.apply(c);
        assert_eq!(m.view().queue.len(), 1);
        assert_eq!(
            m.cluster.as_ref().unwrap().player_state.next_tracks.len(),
            3
        );
        assert_eq!(m.view().repeat, "all");
    }

    #[test]
    fn transferring_to_the_receiver_waits_for_context_resolution_before_another_handoff() {
        let mut m = ConnectModel::new("local".into());
        m.apply(cluster(1));
        let mut before = m.view();
        before.track = Some(ConnectTrack {
            uri: "spotify:track:4uLU6hMCjMI75M1A2tKUQC".into(),
            duration_ms: 1000,
            ..ConnectTrack::default()
        });
        let mut loading = before.clone();
        loading.active_device_id = "local".into();
        loading.queue.clear();
        assert!(!transfer_ready(&loading, &before, "local"));
        loading.queue = before.queue.clone();
        assert!(transfer_ready(&loading, &before, "local"));
        assert!(transfer_ready(&before, &loading, "phone"));
        let mut different = before.clone();
        different.track.as_mut().unwrap().uri = "different".into();
        assert!(!transfer_ready(&different, &before, "phone"));
    }
}
