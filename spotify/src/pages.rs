//! The local pages shown in the browser while connecting.

const STYLE: &str = r#"
body{margin:0;min-height:100vh;display:grid;place-items:center;padding:24px;box-sizing:border-box;
  font-family:"Segoe UI Variable","Segoe UI",system-ui,sans-serif;color:#2e1f0f;
  background:linear-gradient(160deg,#fff6e6,#ffd596)}
main{width:min(620px,100%);background:#fff;border-radius:20px;padding:28px 32px;
  box-shadow:0 20px 60px rgba(120,60,0,.18)}
h1{margin:0 0 6px;font-size:1.6rem;letter-spacing:-.02em}
.lead{color:#8c7558;margin:0 0 18px;line-height:1.5}
ol{padding-left:20px;line-height:1.6}li{margin-bottom:8px}
code{background:#fff3df;border:1px solid #f2e2c6;border-radius:6px;padding:2px 6px;font-size:.95em;user-select:all}
a{color:#b45300;font-weight:600}
form{display:flex;gap:8px;margin-top:18px}
input{flex:1;min-width:0;padding:10px 14px;border-radius:999px;border:1px solid #f2e2c6;font:inherit;outline:none}
input:focus{border-color:#ffa21f;box-shadow:0 0 0 4px rgba(255,162,31,.18)}
button,.btn{border:0;border-radius:999px;padding:10px 20px;font:inherit;font-weight:700;color:#fff;cursor:pointer;
  text-decoration:none;background:linear-gradient(135deg,#ffd25e,#ffa21f 55%,#ff7a1a)}
.ghost{background:#fff;color:#2e1f0f;border:1px solid #f2e2c6}
.error{background:#fdecea;color:#c62828;border-radius:10px;padding:10px 12px}
.note{color:#8c7558;font-size:.85rem;margin-top:16px}
.actions{display:flex;gap:10px;margin-top:18px;flex-wrap:wrap}
"#;

fn page(title: &str, body: &str) -> String {
    format!(
        "<!doctype html><html lang=\"es\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" \
         content=\"width=device-width,initial-scale=1\"><title>{} · Melomango</title><style>{STYLE}</style></head>\
         <body><main>{body}</main></body></html>",
        escape(title)
    )
}

pub fn setup(redirect_uri: &str, current_client_id: &str, invalid: bool) -> String {
    let error = if invalid {
        "<p class=\"error\">Ese Client ID no parece válido: son 32 caracteres (números y letras de la a a la f).</p>"
    } else {
        ""
    };
    let body = format!(
        r#"<h1>🥭 Conecta Melomango con Spotify</h1>
<p class="lead">Melomango solo <strong>lee</strong> tus playlists, tus canciones que te gustan y tus álbumes guardados.
No reproduce música desde Spotify ni puede cambiar nada de tu cuenta.</p>
<p>Necesitas tu propia app de Spotify (es gratis y lleva un par de minutos):</p>
<ol>
  <li>Abre <a href="https://developer.spotify.com/dashboard" target="_blank" rel="noreferrer">developer.spotify.com/dashboard</a>
      e inicia sesión con tu cuenta de Spotify.</li>
  <li>Pulsa <strong>Create app</strong> y pon el nombre y la descripción que quieras (por ejemplo, «Melomango»).</li>
  <li>En <strong>Redirect URIs</strong> añade exactamente <code>{redirect}</code></li>
  <li>Marca <strong>Web API</strong>, acepta las condiciones y guarda.</li>
  <li>En la app que acabas de crear copia el <strong>Client ID</strong> y pégalo aquí:</li>
</ol>
{error}
<form action="/setup" method="get">
  <input name="client_id" value="{current}" placeholder="Client ID (32 caracteres)" required
         pattern="[0-9a-fA-F]{{32}}" autocomplete="off" spellcheck="false" autofocus>
  <button>Conectar</button>
</form>
<p class="note">El Client ID se guarda solo en tu equipo. No hace falta el «Client secret».</p>"#,
        redirect = escape(redirect_uri),
        current = escape(current_client_id),
    );
    page("Conectar Spotify", &body)
}

pub fn success(user: &str) -> String {
    page(
        "Conectado",
        &format!(
            "<h1>✅ ¡Listo, {}!</h1><p class=\"lead\">Melomango ya está conectado con tu cuenta de Spotify.\
             Puedes cerrar esta pestaña y volver a la app para importar tus playlists.</p>",
            escape(user)
        ),
    )
}

pub fn failure(title: &str, detail: &str) -> String {
    page(
        title,
        &format!(
            "<h1>⚠️ {}</h1><p class=\"error\">{}</p><div class=\"actions\">\
             <a class=\"btn\" href=\"/login\">Volver a intentarlo</a>\
             <a class=\"btn ghost\" href=\"/?change=1\">Cambiar Client ID</a></div>",
            escape(title),
            escape(detail)
        ),
    )
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_user_content() {
        let html = failure("Error", "<script>alert(1)</script>");
        assert!(!html.contains("<script>alert"));
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn setup_shows_the_redirect_uri() {
        let html = setup("http://127.0.0.1:43821/callback", "", true);
        assert!(html.contains("http://127.0.0.1:43821/callback"));
        assert!(html.contains("no parece válido"));
    }
}
