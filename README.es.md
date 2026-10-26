# AutoGameSoftwareSync (AGSS)

[English](README.md) | [Français](README.fr.md) | [Русский](README.ru.md) | **Español** | [中文](README.zh.md)

### No vuelvas a perder nada. Protegido, sincronizado, en todas partes.

Tus partidas guardadas y ajustes de juegos y programas, protegidos y sincronizados entre todos tus dispositivos: PC de sobremesa, portátil, consola portátil (Legion Go S, Steam Deck...). Retoma tu partida justo donde la dejaste y recupera la configuración de tus programas estés donde estés. Todo pasa por **tu cofre**: cualquier carpeta compartida que vean todos tus dispositivos (Google Drive, OneDrive, Dropbox, Nextcloud, Syncthing, un NAS, una memoria USB...). Gratis para uso personal, Windows y Linux.

[**Descargar la última versión**](https://github.com/Epikou/AutoGameSoftwareSync/releases/latest) · [Apoya el proyecto en Ko-fi](https://ko-fi.com/autogamesoftwaresave)

![Inicio](screenshots/en/home.png)

## Funciones clave

- **Todo de un vistazo**: una ficha por juego, programa o carpeta, con su estado («A salvo · hace 2 min», «Copiando… 64 %», «Nueva versión de LEGION-GO») y en qué dispositivos está al día.
- **Volver atrás en el tiempo**: una línea de tiempo día a día con todas las versiones de todos tus dispositivos. Recupera la versión de ayer con un clic y **deshazlo** si cambias de idea: siempre se hace antes una copia de seguridad.
- **Tu cofre se encuentra solo**: Google Drive, OneDrive, Dropbox, iCloud, MEGA, pCloud, Nextcloud, Syncthing, unidades de red y USB. Un cofre que ya usas (en otro dispositivo o con una versión anterior) se propone primero.
- **Análisis del PC**: tus juegos (Steam, Epic, GOG, Xbox, Ubisoft, EA, itch, emuladores), tus programas, tus carpetas personales y tus accesos directos de Windows. Lo seguro se marca, el resto se propone; las herramientas de desarrollo y los programas del sistema nunca se proponen.
- **PC nuevo: recupéralo todo**: elige tu dispositivo anterior, comprueba, recupera. Funciona con otro nombre de usuario y de Windows a Linux; un juego aún no instalado se recupera en cuanto aparece.
- **Un primer inicio que de verdad lo configura todo**: 6 pasos cortos (idioma, cofre, este dispositivo, análisis, algo más, primera copia), y luego todo funciona solo.
- **Actualizaciones automáticas y verificadas**: firmadas por el autor, instaladas en un momento tranquilo, y la versión anterior vuelve sola si algo sale mal.
- **Sencillo por defecto, experto si quieres**: palabras sencillas y 6 tarjetas de ajustes; el modo experto conserva todas las opciones técnicas.
- Nunca pierde datos: cada copia es una carpeta nueva, las versiones anteriores se conservan y nada se sobrescribe en tu cofre.
- Detecta tus juegos cuando juegas (y nunca tus herramientas de línea de comandos), navegación completa con mando, modo de pantalla grande para consolas portátiles, temas oscuro y claro. Idiomas: English, Français, Русский, Español, 中文.

| | |
|---|---|
| ![Volver atrás en el tiempo](screenshots/en/time-travel.png) | ![Primer inicio](screenshots/en/first-launch.png) |
| ![Ficha de un elemento](screenshots/en/item.png) | ![Tu cofre](screenshots/en/vault.png) |
| ![PC nuevo](screenshots/en/new-pc.png) | ![Ajustes](screenshots/en/settings.png) |
| ![Pantalla grande](screenshots/en/big-screen.png) | ![Tema claro](screenshots/en/home-light.png) |

## Uso

1. Descarga el archivo desde la página de [Releases](https://github.com/Epikou/AutoGameSoftwareSync/releases/latest), descomprímelo y coloca `AutoGameSoftwareSync.exe` en una carpeta permanente (por ejemplo `C:\Tools\AutoGameSoftwareSync`).
2. Ejecútalo. El primer inicio te guía en 6 pasos: idioma, tu cofre, este dispositivo, el análisis de tu PC, algo más y la primera copia.
3. En tus otros dispositivos, instálalo igual y elige el **mismo cofre**: tus elementos se encuentran y se recuperan.

Si tu cofre está en Google Drive, pon Drive en *Reflejar archivos* (o haz la carpeta *disponible sin conexión*); si no, las lecturas pueden ser muy lentas. La aplicación te avisa cuando detecta el modo «bajo demanda».

## Actualizar desde la 1.3.x

La 2.0.0 es la primera versión con actualización automática: **instálala a mano una vez**. Cierra la versión anterior y sustituye `AutoGameSoftwareSync.exe` por el nuevo, en la misma carpeta. Tus ajustes, tus elementos y tu cofre se conservan tal cual; el primer inicio muestra «Novedades de la 2.0» y solo te pregunta lo nuevo. Hacer una copia manual de tu cofre antes de actualizar siempre es buena idea. A partir de ahí, las actualizaciones se instalan solas (Ajustes > Actualizaciones: Automático / Preguntarme / Nunca).

## Linux / SteamOS

También hay una versión para Linux (x86_64): SteamOS (Steam Deck, Legion Go S...), Bazzite y Ubuntu. Descarga `AutoGameSoftwareSync-v2.0.0-linux-x86_64.tar.gz` desde la página de [Releases](https://github.com/Epikou/AutoGameSoftwareSync/releases/latest), extráelo y ejecuta `install-linux.sh` (no requiere permisos de administrador). Las copias son totalmente compatibles con la versión de Windows a través del mismo cofre, incluidos los juegos de Windows ejecutados con Proton. Guía de instalación (en francés): [docs/LINUX.md](docs/LINUX.md).

![Linux](screenshots/en/linux.png)

## Aviso de Windows SmartScreen

El programa aún no está firmado digitalmente. En el primer inicio, Windows puede mostrar «Windows protegió su PC». Haz clic en **Más información** y luego en **Ejecutar de todas formas**. Este aviso desaparece a medida que más personas usan el programa.

## Privacidad

- Tus partidas guardadas y ajustes van solo al cofre que elijas.
- **Estadísticas de uso opcionales**, solo si haces clic en «Sí, ayudo» (se pregunta en el primer inicio y tras una actualización): como máximo una vez al día, versión, sistema, idioma, funciones usadas, errores por categoría, rangos y los nombres de los títulos conocidos que proteges (de la base integrada y los perfiles), con un identificador aleatorio renovado cada 13 meses. Nunca: los nombres que escribes, archivos, rutas, nombre del dispositivo; la dirección IP no se conserva. Se guardan 90 días, alojadas en Cloudflare. Desactivarlas, ver lo que se envía o borrar tus datos en **Ajustes > Datos de uso**.
- Para un programa desconocido, la aplicación puede hacer una búsqueda web anónima con su nombre. También consulta a GitHub si hay una actualización y, por defecto, la instala automáticamente tras comprobar la firma del autor (Ajustes > Actualizaciones: Automático / Preguntarme / Nunca).
- Detalles (en inglés y francés): [PRIVACY.md](PRIVACY.md).

## Seguridad

Haz una copia manual de tus partidas guardadas antes del primer uso. El programa se ofrece tal cual, sin garantía (ver [TERMS.md](TERMS.md)).

## Limitación conocida

El programa no puede recuperar una partida *antes* de que se inicie un juego lanzado fuera de un lanzador integrado. Para eso se planea una extensión de Playnite.

## Sobre este proyecto

AutoGameSoftwareSync está hecho con **vibe coding**: vibro, luego programo. Apasionado de los juegos y la informática desde hace más de 40 años <3

Está hecho con pasión y no por un gran equipo, así que espera alguna arista (y algún que otro bug que me hará llevarme las manos a la cabeza). Cualquier ayuda, sugerencia, informe de errores o idea es muy bienvenida: abre una [issue](https://github.com/Epikou/AutoGameSoftwareSync/issues). ¡Gracias por estar aquí!

## Soporte

Errores e ideas: abre una [issue](https://github.com/Epikou/AutoGameSoftwareSync/issues). Si te gusta el proyecto, puedes [invitarnos a un café](https://ko-fi.com/autogamesoftwaresave).

*La traducción es automática y puede contener errores.*
