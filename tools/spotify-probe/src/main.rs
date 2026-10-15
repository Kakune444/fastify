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
    let path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("Missing local session file")?;
    let bytes = std::fs::read(path)?;
    let input: Input =
        serde_json::from_slice(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes))?;
    let session = Session::new(SessionConfig::default(), None);
    tokio::time::timeout(
        Duration::from_secs(30),
        session.connect(Credentials::with_access_token(input.access_token), false),
    )
    .await??;
    println!("SESSION_OK");
    let Some(uri) = input.track_uri else {
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
