//! Spotify Web API calls and their mapping to Melomango's collections and tracks.
//! Endpoints: https://developer.spotify.com/documentation/web-api

use std::time::Duration;

use anyhow::{bail, Context};
use serde_json::{json, Value};

use crate::{
    auth::{self, SessionExpired},
    store::{Store, Token},
};

const API: &str = "https://api.spotify.com/v1";

pub struct Me {
    pub id: String,
    pub name: String,
}

pub struct Client<'a> {
    store: &'a Store,
    client_id: String,
    token: Token,
}

impl<'a> Client<'a> {
    pub fn new(store: &'a Store) -> anyhow::Result<Self> {
        let token = store.token().ok_or(SessionExpired)?;
        let client_id = store.config().client_id.ok_or(SessionExpired)?;
        Ok(Self { store, client_id, token })
    }

    fn access_token(&mut self) -> anyhow::Result<String> {
        if self.token.expires_at <= auth::now() + 60 {
            self.token = auth::refresh(self.store, &self.client_id, &self.token)?;
        }
        Ok(self.token.access_token.clone())
    }

    /// GET with automatic token refresh and rate-limit backoff.
    fn get(&mut self, path_or_url: &str) -> anyhow::Result<Value> {
        let url = if path_or_url.starts_with("https://") {
            path_or_url.to_string()
        } else {
            format!("{API}{path_or_url}")
        };
        for attempt in 0..5 {
            let bearer = format!("Bearer {}", self.access_token()?);
            match ureq::get(&url).set("Authorization", &bearer).timeout(Duration::from_secs(20)).call() {
                Ok(resp) => return resp.into_json().context("respuesta no válida de Spotify"),
                Err(ureq::Error::Status(401, _)) if attempt == 0 => self.token.expires_at = 0, // refresh and retry
                Err(ureq::Error::Status(429, resp)) => {
                    let wait: u64 = resp.header("Retry-After").and_then(|v| v.parse().ok()).unwrap_or(2);
                    std::thread::sleep(Duration::from_secs(wait.clamp(1, 15)));
                }
                Err(ureq::Error::Status(403, _)) => {
                    bail!("Spotify no deja leer esta colección (solo playlists tuyas o colaborativas)")
                }
                Err(ureq::Error::Status(code, resp)) => {
                    let body: Value = resp.into_json().unwrap_or(Value::Null);
                    let message = body["error"]["message"].as_str().unwrap_or("error desconocido");
                    bail!("Spotify respondió {code}: {message}");
                }
                Err(e) => bail!("no se pudo conectar con Spotify: {e}"),
            }
        }
        bail!("Spotify está limitando las peticiones; prueba en un momento")
    }

    /// Walks a paging object, calling `f` on every item.
    fn paged(&mut self, first: &str, mut f: impl FnMut(&Value)) -> anyhow::Result<()> {
        let mut page = self.get(first)?;
        loop {
            page["items"].as_array().into_iter().flatten().for_each(&mut f);
            match page["next"].as_str() {
                Some(next) => page = self.get(&next.to_string())?,
                None => return Ok(()),
            }
        }
    }

    pub fn me(&mut self) -> anyhow::Result<Me> {
        let me = self.get("/me")?;
        let id = me["id"].as_str().unwrap_or_default().to_string();
        let name = me["display_name"].as_str().filter(|s| !s.is_empty()).unwrap_or(&id).to_string();
        Ok(Me { id, name })
    }
}

pub fn collections(c: &mut Client) -> anyhow::Result<Value> {
    let me = c.me()?;
    let liked_total = c.get("/me/tracks?limit=1")?["total"].as_u64().unwrap_or(0);
    let mut out = vec![json!({
        "id": "liked",
        "name": "Canciones que te gustan",
        "kind": "liked",
        "owner": me.name,
        "trackCount": liked_total,
        "artworkUrl": null,
        "importable": true,
    })];

    c.paged("/me/playlists?limit=50", |p| {
        let Some(id) = p["id"].as_str() else { return };
        let owner_id = p["owner"]["id"].as_str().unwrap_or_default();
        let collaborative = p["collaborative"].as_bool().unwrap_or(false);
        // `items.total` replaces the deprecated `tracks.total`.
        let count = p["items"]["total"].as_u64().or(p["tracks"]["total"].as_u64()).unwrap_or(0);
        out.push(json!({
            "id": format!("playlist:{id}"),
            "name": p["name"].as_str().unwrap_or("Sin nombre"),
            "kind": "playlist",
            "owner": p["owner"]["display_name"].as_str(),
            "trackCount": count,
            "artworkUrl": pick_image(&p["images"]),
            // The items endpoint only serves playlists you own or collaborate on.
            "importable": owner_id == me.id || collaborative,
        }));
    })?;

    c.paged("/me/albums?limit=50", |item| {
        let album = &item["album"];
        let Some(id) = album["id"].as_str() else { return };
        out.push(json!({
            "id": format!("album:{id}"),
            "name": album["name"].as_str().unwrap_or("Sin nombre"),
            "kind": "album",
            "owner": artist_names(&album["artists"]),
            "trackCount": album["total_tracks"].as_u64().unwrap_or(0),
            "artworkUrl": pick_image(&album["images"]),
            "importable": true,
        }));
    })?;

    Ok(json!({ "collections": out }))
}

pub fn tracks(c: &mut Client, collection_id: &str) -> anyhow::Result<Value> {
    let mut out = Vec::new();
    match collection_id.split_once(':') {
        None if collection_id == "liked" => {
            c.paged("/me/tracks?limit=50", |item| out.extend(map_track(&item["track"], None)))?;
        }
        Some(("playlist", id)) if is_spotify_id(id) => {
            c.paged(&format!("/playlists/{id}/items?limit=50&additional_types=track"), |entry| {
                if entry["is_local"].as_bool() == Some(true) {
                    return;
                }
                // `item` replaces the deprecated `track` field.
                let track = if entry["item"].is_object() { &entry["item"] } else { &entry["track"] };
                if track["type"].as_str() == Some("track") {
                    out.extend(map_track(track, None));
                }
            })?;
        }
        Some(("album", id)) if is_spotify_id(id) => {
            let album = c.get(&format!("/albums/{id}"))?;
            let artwork = pick_image(&album["images"]);
            let mut page = album["tracks"].clone();
            loop {
                for track in page["items"].as_array().into_iter().flatten() {
                    out.extend(map_track(track, artwork.clone()));
                }
                match page["next"].as_str() {
                    Some(next) => page = c.get(&next.to_string())?,
                    None => break,
                }
            }
        }
        _ => bail!("colección desconocida: {collection_id}"),
    }
    Ok(json!({ "tracks": out }))
}

/// Spotify track -> Melomango track. Local files (no id) are skipped.
fn map_track(t: &Value, artwork: Option<String>) -> Option<Value> {
    let id = t["id"].as_str()?;
    let duration_ms = t["duration_ms"].as_f64().unwrap_or(0.0);
    Some(json!({
        "id": id,
        "title": t["name"].as_str().unwrap_or("Sin título"),
        "artist": artist_names(&t["artists"]),
        "durationSecs": (duration_ms / 1000.0).round() as u64,
        "artworkUrl": artwork.or_else(|| pick_image(&t["album"]["images"])),
    }))
}

fn artist_names(artists: &Value) -> String {
    artists
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| a["name"].as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The image closest to 300 px wide (Spotify lists them largest first).
fn pick_image(images: &Value) -> Option<String> {
    images
        .as_array()?
        .iter()
        .min_by_key(|img| (img["width"].as_i64().unwrap_or(640) - 300).abs())
        .and_then(|img| img["url"].as_str())
        .map(str::to_string)
}

fn is_spotify_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_a_playlist_item() {
        let item: Value = serde_json::from_str(
            r#"{"id":"4uLU6hMCjMI75M1A2tKUQC","name":"Dai Dai","duration_ms":223456,"type":"track",
                "artists":[{"name":"Shakira"},{"name":"Burna Boy"}],
                "album":{"images":[{"url":"big","width":640},{"url":"mid","width":300},{"url":"small","width":64}]}}"#,
        )
        .unwrap();
        let t = map_track(&item, None).unwrap();
        assert_eq!(t["artist"], "Shakira, Burna Boy");
        assert_eq!(t["durationSecs"], 223);
        assert_eq!(t["artworkUrl"], "mid");
    }

    #[test]
    fn skips_local_files() {
        let local: Value = serde_json::from_str(r#"{"id":null,"name":"mi.mp3","duration_ms":1000}"#).unwrap();
        assert!(map_track(&local, None).is_none());
    }

    #[test]
    fn validates_ids() {
        assert!(is_spotify_id("37i9dQZF1DXcBWIGoYBM5M"));
        assert!(!is_spotify_id("abc/../me"));
        assert!(!is_spotify_id(""));
    }
}
