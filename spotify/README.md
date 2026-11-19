# Plugin de Spotify para Melomango

Importa en Melomango tus **playlists**, tus **canciones que te gustan** y tus **álbumes guardados** de Spotify.
Solo lee metadatos (permisos `playlist-read-private`, `playlist-read-collaborative` y `user-library-read`):
no reproduce desde Spotify ni modifica tu cuenta. Melomango reproduce cada canción buscándola en YouTube.

> Necesitas **Spotify Premium**: desde febrero de 2026, Spotify solo deja usar apps de desarrollador cuyo dueño
> tenga Premium. Sin Premium, importa tu exportación de datos de Spotify desde Melomango
> (**Importar playlists → Exportación de datos de Spotify**).

## Configuración (una vez)

Al pulsar **Conectar Spotify** en Melomango se abre una página local (`http://127.0.0.1:43821/`) con los pasos:

1. Crea una app en [developer.spotify.com/dashboard](https://developer.spotify.com/dashboard).
2. Añade la Redirect URI **`http://127.0.0.1:43821/callback`** y marca *Web API*.
3. Copia el **Client ID** y pégalo en la página.

El inicio de sesión usa OAuth con PKCE, así que no hace falta el *Client secret*. El Client ID y los tokens se guardan
solo en tu equipo, en la carpeta de datos del plugin (`MELOMANGO_DATA_DIR`). Al desinstalar el plugin se borran.

## Limitaciones de la API de Spotify

- Solo se pueden leer las playlists que has creado o en las que colaboras; las que solo sigues devuelven 403 y
  aparecen como «No disponible».
- Las playlists editoriales y algorítmicas de Spotify no están disponibles para apps nuevas.
- En modo desarrollo, una app de Spotify solo admite a los usuarios que su dueño añade (hasta 5 en apps nuevas) y
  exige que el dueño tenga Premium. Por eso cada persona usa su propia app.

## Probar a mano

```powershell
$env:MELOMANGO_DATA_DIR = "$env:TEMP\spotify-test"
cargo run -- status
cargo run -- connect        # abre el navegador
cargo run -- collections
cargo run -- tracks liked
```
