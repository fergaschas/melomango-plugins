//! Spotify login: Authorization Code with PKCE through a local loopback server, plus
//! token refresh. No client secret is involved.

use std::{
    collections::HashMap,
    fmt,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, bail, Context};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tiny_http::{Header, Request, Response, Server};
use url::Url;

use crate::{
    api, pages,
    store::{Config, Store, Token},
};

pub const PORT: u16 = 43821;
const SCOPES: &str = "playlist-read-private playlist-read-collaborative user-library-read";
const LOGIN_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const TOKEN_URL: &str = "https://accounts.spotify.com/api/token";

pub fn redirect_uri() -> String {
    format!("http://127.0.0.1:{PORT}/callback")
}

/// The saved session no longer works: the user has to connect again.
#[derive(Debug)]
pub struct SessionExpired;

impl fmt::Display for SessionExpired {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "no hay una sesión de Spotify activa: conecta tu cuenta")
    }
}

impl std::error::Error for SessionExpired {}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn status(store: &Store) -> anyhow::Result<Value> {
    if store.token().is_none() {
        return Ok(json!({ "connected": false, "user": null }));
    }
    match api::Client::new(store).and_then(|mut c| c.me()) {
        Ok(me) => Ok(json!({ "connected": true, "user": me.name })),
        Err(e) if e.downcast_ref::<SessionExpired>().is_some() => Ok(json!({ "connected": false, "user": null })),
        Err(e) => Err(e),
    }
}

// ---------- Login ----------

struct Pending {
    verifier: String,
    state: String,
}

/// Opens the browser on a local page that walks the user through setting up the
/// Client ID (first time) and logging in to Spotify. Returns when done.
pub fn connect(store: &Store) -> anyhow::Result<Value> {
    let server = Server::http(("127.0.0.1", PORT)).map_err(|e| {
        anyhow!("no se pudo abrir el puerto {PORT} ({e}); ¿hay otra conexión con Spotify en curso?")
    })?;
    let start = format!("http://127.0.0.1:{PORT}/");
    // MELOMANGO_NO_BROWSER: for tests, don't open the user's browser.
    if std::env::var_os("MELOMANGO_NO_BROWSER").is_some() || open::that_detached(&start).is_err() {
        eprintln!("Abre {start} en tu navegador");
    }

    let deadline = Instant::now() + LOGIN_TIMEOUT;
    let mut pending: Option<Pending> = None;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            bail!("se agotó el tiempo para iniciar sesión en Spotify");
        }
        let Some(request) = server.recv_timeout(left)? else { continue };
        let url = Url::parse(&format!("http://127.0.0.1{}", request.url()))?;
        let query: HashMap<String, String> = url.query_pairs().into_owned().collect();

        match url.path() {
            "/" => {
                if store.config().client_id.is_some() && !query.contains_key("change") {
                    redirect(request, "/login");
                } else {
                    let current = store.config().client_id.unwrap_or_default();
                    html(request, pages::setup(&redirect_uri(), &current, query.contains_key("invalid")));
                }
            }
            "/setup" => {
                let client_id = query.get("client_id").map(|s| s.trim().to_string()).unwrap_or_default();
                if is_client_id(&client_id) {
                    store.save_config(&Config { client_id: Some(client_id) })?;
                    redirect(request, "/login");
                } else {
                    redirect(request, "/?change=1&invalid=1");
                }
            }
            "/login" => {
                let Some(client_id) = store.config().client_id else {
                    redirect(request, "/?change=1");
                    continue;
                };
                let p = Pending { verifier: random_string(48), state: random_string(16) };
                let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(p.verifier.as_bytes()));
                let mut authorize = Url::parse("https://accounts.spotify.com/authorize")?;
                authorize
                    .query_pairs_mut()
                    .append_pair("client_id", &client_id)
                    .append_pair("response_type", "code")
                    .append_pair("redirect_uri", &redirect_uri())
                    .append_pair("code_challenge_method", "S256")
                    .append_pair("code_challenge", &challenge)
                    .append_pair("state", &p.state)
                    .append_pair("scope", SCOPES);
                pending = Some(p);
                redirect(request, authorize.as_str());
            }
            "/callback" => {
                if let Some(error) = query.get("error") {
                    let reason = if error == "access_denied" {
                        "Has cancelado el permiso en Spotify.".to_string()
                    } else {
                        format!("Spotify respondió «{error}».")
                    };
                    html(request, pages::failure("No se completó la conexión", &reason));
                    continue;
                }
                let (Some(code), Some(state), Some(p)) = (query.get("code"), query.get("state"), pending.as_ref()) else {
                    html(request, pages::failure("Enlace caducado", "Empieza de nuevo el inicio de sesión."));
                    continue;
                };
                if *state != p.state {
                    html(request, pages::failure("Enlace caducado", "Empieza de nuevo el inicio de sesión."));
                    continue;
                }
                let client_id = store.config().client_id.unwrap_or_default();
                match exchange_code(&client_id, code, &p.verifier) {
                    Ok(token) => {
                        store.save_token(&token)?;
                        let user = api::Client::new(store)?.me()?.name;
                        html(request, pages::success(&user));
                        return Ok(json!({ "connected": true, "user": user }));
                    }
                    Err(e) => {
                        let detail = format!(
                            "{e:#}. Comprueba que el Client ID es el de tu app y que su Redirect URI es exactamente {}.",
                            redirect_uri()
                        );
                        html(request, pages::failure("Spotify rechazó la conexión", &detail));
                    }
                }
            }
            _ => {
                let _ = request.respond(Response::empty(404));
            }
        }
    }
}

fn redirect(request: Request, location: &str) {
    let header = Header::from_bytes("Location", location).expect("cabecera válida");
    let _ = request.respond(Response::empty(302).with_header(header));
}

fn html(request: Request, body: String) {
    let header = Header::from_bytes("Content-Type", "text/html; charset=utf-8").expect("cabecera válida");
    let _ = request.respond(Response::from_string(body).with_header(header));
}

/// Spotify Client IDs are 32 hex characters.
pub fn is_client_id(s: &str) -> bool {
    s.len() == 32 && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// URL-safe random string (also valid as a PKCE code verifier).
fn random_string(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::getrandom(&mut buf).expect("generador aleatorio del sistema");
    URL_SAFE_NO_PAD.encode(buf)
}

// ---------- Tokens ----------

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: u64,
}

#[derive(Deserialize)]
struct TokenError {
    error: String,
    error_description: Option<String>,
}

fn post_token(form: &[(&str, &str)]) -> Result<TokenResponse, (u16, TokenError)> {
    match ureq::post(TOKEN_URL).send_form(form) {
        Ok(resp) => resp.into_json().map_err(|e| {
            (0, TokenError { error: "respuesta no válida".into(), error_description: Some(e.to_string()) })
        }),
        Err(ureq::Error::Status(code, resp)) => Err((
            code,
            resp.into_json().unwrap_or(TokenError { error: format!("HTTP {code}"), error_description: None }),
        )),
        Err(e) => Err((0, TokenError { error: "sin conexión".into(), error_description: Some(e.to_string()) })),
    }
}

fn exchange_code(client_id: &str, code: &str, verifier: &str) -> anyhow::Result<Token> {
    let redirect = redirect_uri();
    let response = post_token(&[
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", &redirect),
        ("client_id", client_id),
        ("code_verifier", verifier),
    ])
    .map_err(|(_, e)| anyhow!("{}", e.error_description.unwrap_or(e.error)))?;
    Ok(Token {
        access_token: response.access_token,
        refresh_token: response.refresh_token.context("Spotify no devolvió refresh_token")?,
        expires_at: now() + response.expires_in,
    })
}

/// New access token from the refresh token. An invalid grant clears the session.
pub fn refresh(store: &Store, client_id: &str, token: &Token) -> anyhow::Result<Token> {
    match post_token(&[
        ("grant_type", "refresh_token"),
        ("refresh_token", &token.refresh_token),
        ("client_id", client_id),
    ]) {
        Ok(response) => {
            let fresh = Token {
                access_token: response.access_token,
                refresh_token: response.refresh_token.unwrap_or_else(|| token.refresh_token.clone()),
                expires_at: now() + response.expires_in,
            };
            store.save_token(&fresh)?;
            Ok(fresh)
        }
        Err((400, e)) if e.error == "invalid_grant" || e.error == "invalid_client" => {
            store.clear_token()?;
            Err(SessionExpired.into())
        }
        Err((_, e)) => bail!("no se pudo renovar la sesión de Spotify: {}", e.error_description.unwrap_or(e.error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_client_ids() {
        assert!(is_client_id("0123456789abcdef0123456789ABCDEF"));
        assert!(!is_client_id("0123456789abcdef"));
        assert!(!is_client_id("0123456789abcdef0123456789abcdeg"));
    }

    #[test]
    fn verifier_is_pkce_compatible() {
        let v = random_string(48);
        assert_eq!(v.len(), 64);
        assert!(v.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert_ne!(v, random_string(48));
    }
}
