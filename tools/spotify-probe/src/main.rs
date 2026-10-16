//! Local feasibility check. Never send the private input file to CI.
use librespot_core::{
    SpotifyUri, authentication::Credentials, config::SessionConfig, session::Session,
};
use librespot_metadata::{Metadata, Track};
use librespot_playback::{
    audio_backend,
    config::{AudioFormat, PlayerConfig},
    mixer::NoOpVolume,
    player::{Player, PlayerEvent},
};
use protobuf::Message;
use serde::Deserialize;
use std::{path::PathBuf, time::Duration};
#[path = "../../../src-tauri/src/connect.rs"]
mod connect;
#[path = "../../../src-tauri/src/library_api.rs"]
mod library_api;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Input {
    access_token: String,
    track_uri: Option<String>,
}

#[tokio::main(worker_threads = 2)]
async fn main() {
    if run().await.is_err() {
        // The upstream error may embed sensitive authentication data. Keep output generic.
        eprintln!("PROBE_FAILED: session, metadata or native playback could not be validated.");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let stored = std::env::args().any(|arg| arg == "--stored");
    let (credentials, uri) = if stored {
        let saved = keyring::Entry::new("spotisloth-desktop", "spotify-session")?
            .get_password()
            .or_else(|_| {
                keyring::Entry::new("fastify-desktop", "spotify-session")?.get_password()
            })?;
        (serde_json::from_str::<Credentials>(&saved)?, None)
    } else {
        let path = std::env::args_os()
            .nth(1)
            .map(PathBuf::from)
            .ok_or("Missing local session file")?;
        let bytes = std::fs::read(path)?;
        let input: Input =
            serde_json::from_slice(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes))?;
        (
            Credentials::with_access_token(input.access_token),
            input.track_uri,
        )
    };
    let session = Session::new(SessionConfig::default(), None);
    if std::env::args().any(|arg| arg.starts_with("--connect")) {
        return diagnose_connect(session, credentials).await;
    }
    tokio::time::timeout(Duration::from_secs(30), session.connect(credentials, false)).await??;
    println!("SESSION_OK");
    if std::env::args().any(|arg| arg == "--playlist-roundtrip") {
        diagnose_playlist(&session).await?;
    }
    if std::env::args().any(|arg| arg == "--favorites" || arg == "--favorite-roundtrip") {
        diagnose_favorites(&session).await?;
    }
    if std::env::args().any(|arg| arg == "--library") {
        diagnose_library(&session).await?;
    }
    let Some(uri) = uri else {
        session.shutdown();
        return Ok(());
    };
    let track_uri = SpotifyUri::from_uri(&uri)?;
    let track =
        tokio::time::timeout(Duration::from_secs(20), Track::get(&session, &track_uri)).await??;
    println!("METADATA_OK: duration_ms={}", track.duration);
    if !std::env::args().any(|arg| arg == "--play") {
        session.shutdown();
        return Ok(());
    }
    let backend = audio_backend::find(Some("rodio".to_owned())).ok_or("No audio backend")?;
    let player = Player::new(
        PlayerConfig::default(),
        session.clone(),
        Box::new(NoOpVolume),
        move || backend(None, AudioFormat::default()),
    );
    let mut events = player.get_player_event_channel();
    player.load(track_uri, true, 0);
    let result = tokio::time::timeout(Duration::from_secs(30), async {
        while let Some(event) = events.recv().await {
            match event {
                PlayerEvent::Playing { .. } => {
                    println!("NATIVE_PLAYBACK_STARTED");
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    return Ok::<(), &'static str>(());
                }
                PlayerEvent::Unavailable { .. } => return Err("Audio unavailable"),
                _ => {}
            }
        }
        Err("Player closed")
    })
    .await;
    player.stop();
    session.shutdown();
    result??;
    println!("PROBE_OK");
    Ok(())
}

async fn diagnose_connect(
    session: Session,
    credentials: Credentials,
) -> Result<(), Box<dyn std::error::Error>> {
    use librespot_playback::mixer::{Mixer, MixerConfig, softmixer::SoftMixer};
    use std::sync::Arc;
    let mixer = SoftMixer::open(MixerConfig::default())?;
    let backend = audio_backend::find(Some("rodio".to_owned())).ok_or("No audio backend")?;
    let player = Player::new(
        PlayerConfig::default(),
        session.clone(),
        mixer.get_soft_volume(),
        move || backend(None, AudioFormat::default()),
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let runtime = connect::ConnectRuntime::start(
        session.clone(),
        credentials,
        player,
        Arc::new(mixer),
        client,
    )
    .await
    .map_err(|_| "Connect initialization failed")?;
    println!("CONNECT_SESSION_OK");
    let mut updates = runtime.subscribe();
    let result = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let view = runtime.view().await;
            if view.ready && view.devices.iter().any(|d| !d.local) {
                println!(
                    "CONNECT_DEVICES_OK: devices={} remote={} active={} queue={}",
                    view.devices.len(),
                    view.devices.iter().filter(|d| !d.local).count(),
                    !view.active_device_id.is_empty(),
                    view.queue.len()
                );
                return Ok::<(), &'static str>(());
            }
            updates
                .changed()
                .await
                .map_err(|_| "Connect stream closed")?;
        }
    })
    .await;
    let view = runtime.view().await;
    println!("CONNECT_QUEUE_SHAPE: {}", runtime.queue_diagnostics().await);
    if result.is_err() {
        println!(
            "CONNECT_NO_REMOTE: ready={} devices={} active={}",
            view.ready,
            view.devices.len(),
            !view.active_device_id.is_empty()
        );
    }
    let actions = if result.is_ok() {
        diagnose_connect_actions(&runtime).await
    } else {
        Ok(())
    };
    if let Err(reason) = &actions {
        // These errors are built above from static messages and the sanitized ConnectError enum.
        println!("CONNECT_ACTION_FAILED: {reason}");
    }
    runtime.shutdown();
    tokio::time::sleep(Duration::from_millis(400)).await;
    session.shutdown();
    result.map_err(|_| "Connect device updates were not confirmed")??;
    actions?;
    Ok(())
}

async fn wait_connect(
    runtime: &connect::ConnectRuntime,
    predicate: impl Fn(&connect::ConnectView) -> bool,
) -> Result<connect::ConnectView, Box<dyn std::error::Error>> {
    let mut updates = runtime.subscribe();
    Ok(tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let view = runtime.view().await;
            if predicate(&view) {
                return Ok::<_, &'static str>(view);
            }
            updates
                .changed()
                .await
                .map_err(|_| "Connect stream closed")?;
        }
    })
    .await
    .map_err(|_| "Connect state confirmation timed out")??)
}

async fn diagnose_connect_actions(
    runtime: &connect::ConnectRuntime,
) -> Result<(), Box<dyn std::error::Error>> {
    use connect::ConnectAction;
    if std::env::args().any(|a| a == "--connect-transfer-roundtrip") {
        let initial = runtime.view().await;
        if initial.active_device_id.is_empty() || initial.track.is_none() {
            return Err("An active playback is required".into());
        }
        runtime
            .execute(
                ConnectAction::Transfer {
                    device_id: initial.local_device_id.clone(),
                },
                Some(&initial.active_device_id),
            )
            .await
            .map_err(|e| format!("Connect transfer in: {e:?}"))?;
        println!("CONNECT_TRANSFER_IN_OK");
        let local = runtime.view().await;
        runtime
            .execute(
                ConnectAction::Transfer {
                    device_id: initial.active_device_id.clone(),
                },
                Some(&local.active_device_id),
            )
            .await
            .map_err(|e| format!("Connect transfer out: {e:?}"))?;
        println!("CONNECT_TRANSFER_OUT_OK");
    }
    if std::env::args().any(|a| a == "--connect-queue-roundtrip") {
        let initial = runtime.view().await;
        let uri = initial
            .track
            .as_ref()
            .ok_or("An active track is required")?
            .uri
            .clone();
        let before = initial.queue.iter().filter(|t| t.track.uri == uri).count();
        let uids = initial
            .queue
            .iter()
            .map(|t| {
                t.key
                    .split_once(':')
                    .map(|(_, uid)| uid)
                    .unwrap_or("")
                    .to_owned()
            })
            .collect::<std::collections::HashSet<_>>();
        runtime
            .execute(
                ConnectAction::Add { uri: uri.clone() },
                Some(&initial.active_device_id),
            )
            .await
            .map_err(|e| format!("Connect queue add: {e:?}"))?;
        let changed = wait_connect(runtime, |v| {
            v.queue.iter().filter(|t| t.track.uri == uri).count() == before + 1
        })
        .await;
        println!(
            "CONNECT_QUEUE_SHAPE_AFTER: {}",
            runtime.queue_diagnostics().await
        );
        let changed = changed?;
        println!("CONNECT_QUEUE_ADD_OK");
        let candidates = changed
            .queue
            .iter()
            .filter(|t| {
                t.track.uri == uri
                    && t.key
                        .split_once(':')
                        .is_some_and(|(_, uid)| !uid.is_empty() && !uids.contains(uid))
            })
            .collect::<Vec<_>>();
        if candidates.len() != 1 {
            return Err("New queue occurrence could not be identified safely".into());
        }
        runtime
            .execute(
                ConnectAction::Remove {
                    key: candidates[0].key.clone(),
                    revision: changed.revision.clone(),
                },
                Some(&changed.active_device_id),
            )
            .await
            .map_err(|e| format!("Connect queue cleanup: {e:?}"))?;
        wait_connect(runtime, |v| {
            v.queue.iter().filter(|t| t.track.uri == uri).count() == before
        })
        .await?;
        println!("CONNECT_QUEUE_REMOVE_OK");
    }
    Ok(())
}

async fn diagnose_favorites(session: &Session) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let api = library_api::LibraryApi::new(session, &client);
    let (uris, next) = api.liked_page("").await.map_err(|e| format!("{e:?}"))?;
    let (bulk, _) = api
        .liked_uris_page("", 1000)
        .await
        .map_err(|e| format!("{e:?}"))?;
    if !bulk.is_empty()
        && api
            .contains(&bulk.iter().take(3).cloned().collect::<Vec<_>>())
            .await
            .map_err(|e| format!("{e:?}"))?
            .iter()
            .any(|saved| !saved)
    {
        return Err("Context items are not saved tracks".into());
    }
    println!(
        "LIKED_CONTEXT_OK: tracks={} identifiers_only=true",
        bulk.len()
    );
    println!(
        "LIKED_PAGE_OK: tracks={} has_next={}",
        uris.len(),
        next.is_some()
    );
    let samples: Vec<String> = [
        "spotify:track:2cGxRwrMyEAp8dEbuZaVv6",
        "spotify:track:2LD2gT7gwAurzdQDQtILds",
        "spotify:track:69kOkLUCkxIZYexIgSG8rq",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    let saved = api.contains(&samples).await.map_err(|e| format!("{e:?}"))?;
    println!("LIKED_STATUS_OK: checked={}", saved.len());
    if !std::env::args().any(|arg| arg == "--favorite-roundtrip") {
        return Ok(());
    }
    let index = saved
        .iter()
        .position(|saved| !saved)
        .ok_or("All candidate tracks are already liked; leave them untouched")?;
    let uri = &samples[index];
    let added = api.set_saved(uri, true).await;
    // This candidate was absent beforehand. Restore that state even after an uncertain response.
    let restored = api.set_saved(uri, false).await;
    if restored.is_err() {
        println!("FAVORITE_RESTORE_FAILED");
        return Err("Favorite test cleanup failed".into());
    }
    added.map_err(|e| format!("{e:?}"))?;
    println!("FAVORITE_ROUNDTRIP_OK: add_and_remove_confirmed=true initial_state_restored=true");
    Ok(())
}

async fn diagnose_playlist(session: &Session) -> Result<(), Box<dyn std::error::Error>> {
    use library_api::{LibraryApi, PlaylistEdit, revision_hex};
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let api = LibraryApi::new(session, &client);
    let uri = api
        .create_playlist(
            "Spotisloth — test édition",
            "Validation temporaire de Spotisloth.",
        )
        .await
        .map_err(|e| format!("{e:?}"))?;
    println!("PLAYLIST_CREATED_PRIVATE");
    if let Some(directory) = std::env::var_os("SPOTISLOTH_DIAGNOSTIC_DIR") {
        std::fs::create_dir_all(&directory)?;
        std::fs::write(PathBuf::from(directory).join("test-playlist-uri.txt"), &uri)?;
    }
    let first = "spotify:track:2cGxRwrMyEAp8dEbuZaVv6".to_owned();
    let second = "spotify:track:2LD2gT7gwAurzdQDQtILds".to_owned();
    let result = async {
        let page = api.playlist_page(&uri, 0, 10).await?;
        let stale_revision = revision_hex(page.revision());
        api.edit_playlist(
            &uri,
            Some(&stale_revision),
            &PlaylistEdit::Details {
                name: "Spotisloth — test renommé".into(),
                description: "Description modifiée.".into(),
            },
        )
        .await?;
        println!("PLAYLIST_DETAILS_OK");
        api.edit_playlist(
            &uri,
            None,
            &PlaylistEdit::Add {
                uris: vec![first.clone(), second.clone(), first.clone()],
            },
        )
        .await?;
        println!("PLAYLIST_ADD_OK");
        let page = api.playlist_page(&uri, 0, 10).await?;
        api.edit_playlist(
            &uri,
            Some(&revision_hex(page.revision())),
            &PlaylistEdit::Remove {
                position: 2,
                uri: first.clone(),
            },
        )
        .await?;
        let page = api.playlist_page(&uri, 0, 10).await?;
        if page.length() != 2
            || page
                .contents
                .as_ref()
                .and_then(|c| c.items.first())
                .is_none_or(|i| i.uri() != first)
        {
            return Err(library_api::LibraryError::Protocol);
        }
        println!("PLAYLIST_REMOVE_ONE_DUPLICATE_OK");
        api.edit_playlist(
            &uri,
            Some(&revision_hex(page.revision())),
            &PlaylistEdit::Move {
                position: 0,
                to: 1,
                uri: first.clone(),
            },
        )
        .await?;
        println!("PLAYLIST_MOVE_OK");
        if !matches!(
            api.edit_playlist(
                &uri,
                Some(&stale_revision),
                &PlaylistEdit::Add {
                    uris: vec![first.clone()]
                }
            )
            .await,
            Err(library_api::LibraryError::Conflict)
        ) {
            return Err(library_api::LibraryError::Protocol);
        }
        println!("PLAYLIST_STALE_REVISION_BLOCKED");
        let page = api.playlist_page(&uri, 0, 10).await?;
        api.edit_playlist(
            &uri,
            Some(&revision_hex(page.revision())),
            &PlaylistEdit::Remove {
                position: 1,
                uri: first.clone(),
            },
        )
        .await?;
        let page = api.playlist_page(&uri, 0, 10).await?;
        api.edit_playlist(
            &uri,
            Some(&revision_hex(page.revision())),
            &PlaylistEdit::Remove {
                position: 0,
                uri: second.clone(),
            },
        )
        .await?;
        Ok::<(), library_api::LibraryError>(())
    }
    .await;
    let cleanup = api.unlink_playlist(&uri).await;
    if cleanup.is_err() {
        println!("PLAYLIST_TEST_CLEANUP_FAILED");
        return Err("Test playlist cleanup failed".into());
    }
    println!("PLAYLIST_TEST_REMOVED_FROM_LIBRARY");
    result.map_err(|e| format!("{e:?}"))?;
    println!("PLAYLIST_ROUNDTRIP_OK");
    Ok(())
}

async fn diagnose_library(session: &Session) -> Result<(), Box<dyn std::error::Error>> {
    let bytes = tokio::time::timeout(
        Duration::from_secs(15),
        session.spclient().get_rootlist(0, Some(40)),
    )
    .await??;
    let root =
        librespot_protocol::playlist4_external::SelectedListContent::parse_from_bytes(&bytes)?;
    let items: Vec<_> = root
        .contents
        .as_ref()
        .into_iter()
        .flat_map(|c| &c.items)
        .collect();
    let uris: Vec<_> = items
        .iter()
        .filter_map(|item| {
            let uri = SpotifyUri::from_uri(item.uri()).ok()?;
            matches!(uri, SpotifyUri::Playlist { .. })
                .then(|| uri.to_uri())
                .and_then(Result::ok)
        })
        .collect();
    println!(
        "ROOTLIST_OK: total={} items={} playlists={} diff={} decorated={}",
        root.length(),
        items.len(),
        uris.len(),
        root.diff.is_some(),
        root.contents.as_ref().map_or(0, |c| c
            .meta_items
            .iter()
            .filter(|m| m.attributes.as_ref().is_some_and(|a| !a.name().is_empty()))
            .count())
    );
    let access = session.login5().auth_token().await?;
    let client = session.spclient().client_token().await?;
    let definitions: serde_json::Value =
        serde_json::from_str(include_str!("../../../src-tauri/src/queries.json"))?;
    let mut body = definitions["lookupChildEntities"].clone();
    body["variables"] = serde_json::json!({"uris":uris});
    let response: serde_json::Value = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?
        .post("https://api-partner.spotify.com/pathfinder/v2/query")
        .bearer_auth(access.access_token)
        .header("client-token", client)
        .header("app-platform", "Win32")
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    println!(
        "LOOKUP_OK: entities={} errors={}",
        response["data"]["lookupEntities"]
            .as_array()
            .map_or(0, Vec::len),
        response["errors"].as_array().map_or(0, Vec::len)
    );
    // Explicit opt-in: this file contains private library metadata and must stay local.
    if let Some(directory) = std::env::var_os("SPOTISLOTH_DIAGNOSTIC_DIR") {
        std::fs::create_dir_all(&directory)?;
        std::fs::write(
            PathBuf::from(directory).join("library.json"),
            serde_json::to_vec(&response)?,
        )?;
    }
    Ok(())
}
