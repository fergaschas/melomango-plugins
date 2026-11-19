//! Melomango plugin for YouTube, built on top of yt-dlp.
//!
//! Protocol: https://github.com/fergaschas/melomango/blob/main/plugins/PROTOCOL.md
//!   melomango-youtube search <query>   -> {"tracks": [...]}
//!   melomango-youtube trending         -> {"tracks": [...]}
//!   melomango-youtube stream <id>      -> {"url": "...", "headers": {...}}
//!
//! yt-dlp is looked up in this order: $MELOMANGO_YTDLP, next to this executable,
//! the WinGet links folder (Windows), and finally the PATH.

mod clean;

use std::{
    env,
    path::PathBuf,
    process::{Command, Stdio},
};

use anyhow::{bail, Context};
use serde::Serialize;
use serde_json::{json, Value};

const SEARCH_RESULTS: u32 = 25;
/// Weekly "Top 100 Songs Global" chart published by YouTube Music.
const TRENDING_PLAYLIST: &str = "https://www.youtube.com/playlist?list=PL4fGSI1pDJn6puJdseH2Rt9sMvt9E2M4i";
/// Longer results are usually hour-long mixes, not songs.
const MAX_DURATION_SECS: f64 = 20.0 * 60.0;
/// Prefer AAC in MP4: every WebView plays it.
const AUDIO_FORMAT: &str = "bestaudio[ext=m4a]/bestaudio[acodec^=mp4a]/bestaudio";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Track {
    id: String,
    title: String,
    artist: String,
    duration_secs: u32,
    artwork_url: String,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e:#}");
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    let output = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["search", query] => tracks(&ytdlp_json(&[
            "--flat-playlist",
            &format!("ytsearch{SEARCH_RESULTS}:{query}"),
        ])?),
        ["trending"] => tracks(&ytdlp_json(&[
            "--flat-playlist",
            "--playlist-end",
            "50",
            TRENDING_PLAYLIST,
        ])?),
        ["stream", id] => stream(id)?,
        _ => bail!("uso: melomango-youtube <search QUERY | trending | stream ID>"),
    };
    println!("{output}");
    Ok(())
}

fn tracks(result: &Value) -> Value {
    let tracks: Vec<Track> = result["entries"]
        .as_array()
        .map(|entries| entries.iter().filter_map(to_track).collect())
        .unwrap_or_default();
    json!({ "tracks": tracks })
}

fn to_track(entry: &Value) -> Option<Track> {
    let id = entry["id"].as_str()?;
    if !is_video_id(id) || matches!(entry["live_status"].as_str(), Some("is_live" | "is_upcoming")) {
        return None;
    }
    let duration = entry["duration"].as_f64().filter(|d| *d > 0.0 && *d <= MAX_DURATION_SECS)?;
    let channel = entry["channel"]
        .as_str()
        .or(entry["uploader"].as_str())
        .unwrap_or("YouTube");
    let (artist, title) = clean::artist_and_title(entry["title"].as_str()?, channel);
    Some(Track {
        id: id.to_string(),
        title,
        artist,
        duration_secs: duration.round() as u32,
        artwork_url: format!("https://i.ytimg.com/vi/{id}/hqdefault.jpg"),
    })
}

fn stream(id: &str) -> anyhow::Result<Value> {
    if !is_video_id(id) {
        bail!("id de vídeo no válido: {id}");
    }
    let info = ytdlp_json(&[
        "--no-playlist",
        "-f",
        AUDIO_FORMAT,
        &format!("https://www.youtube.com/watch?v={id}"),
    ])?;
    let url = info["url"].as_str().context("yt-dlp no devolvió URL de audio")?;
    Ok(json!({ "url": url, "headers": info["http_headers"] }))
}

fn is_video_id(id: &str) -> bool {
    id.len() == 11 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn ytdlp_json(args: &[&str]) -> anyhow::Result<Value> {
    let ytdlp = find_ytdlp().context(
        "no se encontró yt-dlp. Instálalo con `winget install yt-dlp.yt-dlp` \
         o copia yt-dlp en la carpeta del plugin",
    )?;
    let mut cmd = Command::new(&ytdlp);
    cmd.args(["-J", "--no-warnings", "--ignore-config", "--encoding", "utf-8"])
        .args(args)
        .stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let output = cmd
        .output()
        .with_context(|| format!("no se pudo ejecutar {}", ytdlp.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let message = stderr.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("error desconocido");
        bail!("yt-dlp: {}", message.trim());
    }
    serde_json::from_slice(&output.stdout).context("yt-dlp devolvió JSON no válido")
}

fn find_ytdlp() -> Option<PathBuf> {
    let exe_name = if cfg!(windows) { "yt-dlp.exe" } else { "yt-dlp" };
    let candidates = [
        env::var_os("MELOMANGO_YTDLP").map(PathBuf::from),
        env::current_exe().ok().and_then(|p| Some(p.parent()?.join(exe_name))),
        env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Microsoft/WinGet/Links").join(exe_name)),
    ];
    if let Some(found) = candidates.into_iter().flatten().find(|p| p.is_file()) {
        return Some(found);
    }
    // Fall back to the PATH.
    let path = env::var_os("PATH")?;
    env::split_paths(&path).map(|dir| dir.join(exe_name)).find(|p| p.is_file())
}
