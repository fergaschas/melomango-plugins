//! yt-dlp as an installable requirement ("requirements" capability).
//!
//!   melomango-youtube requirements                -> {"requirements": [Requirement]}
//!   melomango-youtube install-requirement yt-dlp  -> {"requirements": [Requirement]} once installed
//!
//! On Windows the plugin downloads its own, faster copy of yt-dlp (see `managed`), with
//! winget as a fallback; elsewhere the user installs yt-dlp by hand.

use std::process::Command;

use anyhow::{bail, Context};
use serde_json::{json, Value};

use crate::{find_ytdlp, managed, managed::hidden};

const WINGET_ID: &str = "yt-dlp.yt-dlp";

pub fn list() -> Value {
    json!({
        "requirements": [{
            "id": "yt-dlp",
            "name": "yt-dlp",
            "description": "Programa libre que obtiene el audio de YouTube. El plugin lo descarga de github.com/yt-dlp y lo mantiene al día.",
            "installed": find_ytdlp().is_some(),
            "installable": managed::supported() || winget_available(),
        }]
    })
}

pub fn install(id: &str) -> anyhow::Result<Value> {
    if id != "yt-dlp" {
        bail!("requisito desconocido: {id}");
    }
    if find_ytdlp().is_some() {
        return Ok(list());
    }
    let managed_error = match managed::supported().then(managed::update) {
        Some(Ok(_)) => return Ok(list()),
        Some(Err(e)) => Some(e),
        None => None,
    };
    if !winget_available() {
        if let Some(e) = managed_error {
            return Err(e.context("no se pudo descargar yt-dlp"));
        }
        bail!("winget no está disponible: instala yt-dlp a mano (https://github.com/yt-dlp/yt-dlp)");
    }
    let output = hidden(Command::new("winget").args([
        "install",
        "--id",
        WINGET_ID,
        "--exact",
        "--silent",
        "--accept-package-agreements",
        "--accept-source-agreements",
        "--disable-interactivity",
    ]))
    .output()
    .context("no se pudo ejecutar winget")?;

    // winget's exit code is not reliable (e.g. "already installed"): check the result instead.
    if find_ytdlp().is_none() {
        let log = String::from_utf8_lossy(&output.stdout);
        let reason = log
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && l.chars().any(char::is_alphabetic))
            .last()
            .unwrap_or("sin detalles");
        bail!("winget no pudo instalar yt-dlp: {reason}");
    }
    Ok(list())
}

fn winget_available() -> bool {
    cfg!(windows)
        && hidden(Command::new("winget").arg("--version"))
            .output()
            .is_ok_and(|o| o.status.success())
}
