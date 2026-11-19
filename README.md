# 🥭 Plugins de Melomango

Repositorio oficial de plugins para [Melomango](https://github.com/fergaschas/melomango), el reproductor de
música en streaming gratuito.

La app lee [`index.json`](index.json) y muestra estos plugins en **Plugins → Explorar**, desde donde se instalan
con un clic. Cada descarga se comprueba con su huella SHA-256 antes de instalarse.

## Plugins

| Plugin | Descripción | Plataformas |
|---|---|---|
| [YouTube](youtube) | Todo el catálogo de YouTube, vía [yt-dlp](https://github.com/yt-dlp/yt-dlp). Requiere `winget install yt-dlp.yt-dlp`. | Windows x64 |

> ⚖️ Reproducir YouTube fuera de su reproductor va contra sus Términos de Servicio. Úsalo para uso personal y
> bajo tu responsabilidad.

## Cómo funciona

- Cada carpeta (`youtube/`, …) es el código de un plugin con su `plugin.json`.
- Los binarios se publican como *assets* de una release `<id>-v<versión>` en este repo.
- `index.json` lista los plugins con la URL y el SHA-256 de cada descarga por plataforma
  (`windows-x86_64`, `macos-aarch64`, `linux-x86_64`, …).

El protocolo de plugins y el formato del índice están en
[PROTOCOL.md](https://github.com/fergaschas/melomango/blob/main/plugins/PROTOCOL.md).

## Publicar una versión

Requisitos: Rust, Node.js y [GitHub CLI](https://cli.github.com) con sesión iniciada.

```powershell
node scripts/release.mjs youtube   # compila, sube el zip a la release y actualiza index.json
git commit -am "youtube 0.1.0" ; git push
```

Para añadir otra plataforma a la misma versión, ejecuta el script en ese sistema operativo: añade su descarga
sin borrar las demás.

## Otros repositorios

Cualquiera puede publicar su propio `index.json` (por HTTPS) y añadirlo en la app desde
**Plugins → Explorar → Repositorios**.
