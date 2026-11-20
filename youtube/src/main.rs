//! Melomango plugin for YouTube, built on top of yt-dlp.
//!
//! Protocol: https://github.com/fergaschas/melomango/blob/main/plugins/PROTOCOL.md
//!   melomango-youtube search <query>   -> {"tracks": [...]}
//!   melomango-youtube trending         -> {"tracks": [...]}
//!   melomango-youtube stream <id>      -> {"url": "...", "headers": {...}}
//!   melomango-youtube radio <id>       -> {"tracks": [...]} songs like it (YouTube's "Mix")
//!   melomango-youtube playlist <id>    -> {"title": "...", "tracks": [...]} a YouTube (Music) playlist
//!
//! yt-dlp is looked up in this order: $MELOMANGO_YTDLP, the copy this plugin manages
//! (see `managed`), next to this executable, WinGet's folders (Windows), and the PATH.

mod clean;
mod managed;
mod requirements;

use std::{
    env,
    path::PathBuf,
    process::{Command, Stdio},
};

use anyhow::{bail, Context};
use serde::Serialize;
use serde_json::{json, Value};

const SEARCH_RESULTS: u32 = 25;
/// Songs taken from a video's "Mix" playlist for the radio.
const RADIO_RESULTS: u32 = 40;
/// Weekly "Top 100 Songs Global" chart published by YouTube Music.
const TRENDING_PLAYLIST: &str = "https://www.youtube.com/playlist?list=PL4fGSI1pDJn6puJdseH2Rt9sMvt9E2M4i";
/// Longer results are usually hour-long mixes, not songs.
const MAX_DURATION_SECS: f64 = 20.0 * 60.0;
/// Prefer AAC in MP4: every WebView plays it.
const AUDIO_FORMAT: &str = "bestaudio[ext=m4a]/bestaudio[acodec^=mp4a]/bestaudio";
/// The visionOS client is ~1.5 s faster to extract and its URLs work immediately (the
/// default clients' URLs answer 403 for ~2 s). Skipping the HLS manifest saves ~0.4 s more:
/// the audio comes as a plain HTTPS format. yt-dlp's defaults remain the fallback.
const FAST_CLIENT: &str = "youtube:player_client=visionos;skip=hls";

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
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    if matches!(args.first(), Some(&("search" | "trending" | "stream" | "radio" | "playlist"))) {
        managed::update_in_background();
    }
    let output = match args.as_slice() {
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
        ["radio", id] => radio(id)?,
        ["playlist", id] => playlist(id)?,
        ["requirements"] => requirements::list(),
        ["install-requirement", id] => requirements::install(id)?,
        ["update-ytdlp"] => json!({ "path": managed::update()? }),
        _ => bail!(
            "uso: melomango-youtube <search QUERY | trending | stream ID | radio ID | playlist ID | requirements | install-requirement ID | update-ytdlp>"
        ),
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
    let url = format!("https://www.youtube.com/watch?v={id}");
    let base = ["--no-playlist", "-f", AUDIO_FORMAT];
    let fast = [&base[..], &["--extractor-args", FAST_CLIENT, &url]].concat();
    let info = match ytdlp_json(&fast) {
        Ok(info) if info["url"].is_string() => info,
        _ => ytdlp_json(&[&base[..], &[url.as_str()]].concat())?,
    };
    let url = info["url"].as_str().context("yt-dlp no devolvió URL de audio")?;
    Ok(json!({ "url": url, "headers": info["http_headers"] }))
}

/// Songs like `id`, from the "Mix" playlist YouTube builds for every video (without the
/// video itself).
fn radio(id: &str) -> anyhow::Result<Value> {
    if !is_video_id(id) {
        bail!("id de vídeo no válido: {id}");
    }
    let mix = ytdlp_json(&[
        "--flat-playlist",
        "--playlist-end",
        &RADIO_RESULTS.to_string(),
        &format!("https://www.youtube.com/watch?v={id}&list=RD{id}"),
    ])?;
    let mut result = tracks(&mix);
    if let Some(list) = result["tracks"].as_array_mut() {
        list.retain(|t| t["id"].as_str() != Some(id));
    }
    Ok(result)
}

/// The songs of a YouTube or YouTube Music playlist (also albums, `OLAK5uy_…`).
fn playlist(id: &str) -> anyhow::Result<Value> {
    if id.is_empty() || id.len() > 64 || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        bail!("id de playlist no válido: {id}");
    }
    let list = ytdlp_json(&["--flat-playlist", &format!("https://www.youtube.com/playlist?list={id}")])?;
    let mut result = tracks(&list);
    let title = list["title"].as_str().unwrap_or("Playlist de YouTube").trim_start_matches("Album - ");
    result["title"] = json!(title);
    Ok(result)
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
        managed::executable(),
        env::current_exe().ok().and_then(|p| Some(p.parent()?.join(exe_name))),
        env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Microsoft/WinGet/Links").join(exe_name)),
    ];
    if let Some(found) = candidates.into_iter().flatten().find(|p| p.is_file()) {
        return Some(found);
    }
    // Without permission to create the Links shortcut, winget leaves it in its package folder
    // and only updates the PATH of new sessions.
    if let Some(local) = env::var_os("LOCALAPPDATA") {
        let packages = PathBuf::from(local).join("Microsoft/WinGet/Packages");
        if let Ok(entries) = std::fs::read_dir(packages) {
            let found = entries
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with("yt-dlp.yt-dlp_"))
                .map(|e| e.path().join(exe_name))
                .find(|p| p.is_file());
            if found.is_some() {
                return found;
            }
        }
    }
    // Fall back to the PATH.
    let path = env::var_os("PATH")?;
    env::split_paths(&path).map(|dir| dir.join(exe_name)).find(|p| p.is_file())
}
