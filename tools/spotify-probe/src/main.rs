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
        let saved = keyring::Entry::new("fastify-desktop", "spotify-session")?.get_password()?;
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
    tokio::time::timeout(Duration::from_secs(30), session.connect(credentials, false)).await??;
    println!("SESSION_OK");
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
    if let Some(directory) = std::env::var_os("FASTIFY_DIAGNOSTIC_DIR") {
        std::fs::create_dir_all(&directory)?;
        std::fs::write(
            PathBuf::from(directory).join("library.json"),
            serde_json::to_vec(&response)?,
        )?;
    }
    Ok(())
}
