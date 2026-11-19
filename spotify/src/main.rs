//! Melomango plugin for Spotify: imports playlists, liked songs and saved albums.
//! Metadata only — Melomango plays the songs from another source (YouTube).
//!
//! Protocol (library capability): https://github.com/fergaschas/melomango/blob/main/plugins/PROTOCOL.md
//!   melomango-spotify status | connect | disconnect   -> {"connected": bool, "user": "..."}
//!   melomango-spotify collections                     -> {"collections": [...]}
//!   melomango-spotify tracks <collection id>          -> {"tracks": [...]}
//!
//! Each user brings their own Spotify app (Client ID), set up on first `connect`.
//! Settings and tokens live in $MELOMANGO_DATA_DIR.

mod api;
mod auth;
mod pages;
mod store;

use anyhow::bail;
use serde_json::json;

fn main() {
    if let Err(e) = run() {
        eprintln!("{e:#}");
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let store = store::Store::open()?;
    let output = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["status"] => auth::status(&store)?,
        ["connect"] => auth::connect(&store)?,
        ["disconnect"] => {
            store.clear_token()?;
            json!({ "connected": false, "user": null })
        }
        ["collections"] => api::collections(&mut api::Client::new(&store)?)?,
        ["tracks", id] => api::tracks(&mut api::Client::new(&store)?, id)?,
        _ => bail!("uso: melomango-spotify <status | connect | disconnect | collections | tracks ID>"),
    };
    println!("{output}");
    Ok(())
}
