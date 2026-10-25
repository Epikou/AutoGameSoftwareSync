# AutoGameSoftwareSync (AGSS)

**English** | [Français](README.fr.md) | [Русский](README.ru.md) | [Español](README.es.md) | [中文](README.zh.md)

Your saves and settings for games and software, from one PC to another (desktop, handheld such as the Legion Go S...) through any shared folder: Google Drive, Dropbox, a NAS, etc. Free for personal use.

[**Download the latest version**](https://github.com/Epikou/AutoGameSoftwareSync/releases/latest) · [Support the project on Ko-fi](https://ko-fi.com/autogamesoftwaresave)

## What it does

- Backs up a game's saves when you close the game, and restores the most recent version on your other PC.
- Never loses data: every save is a new, unique folder; older versions are always kept; a safety copy is made before any restore.
- Finds your games (Steam, Epic, GOG) and knows thousands of save locations. Manual add is also available.
- Runs quietly in the system tray, can start with Windows (including the Xbox full-screen experience on handhelds).
- Full gamepad navigation (DualSense, Xbox controllers). Languages: English, Français, Русский, Español, 中文.

## How to use

1. Download `AutoGameSoftwareSync.exe` from the [Releases](https://github.com/Epikou/AutoGameSoftwareSync/releases) page and put it in a permanent folder (for example `C:\Tools\AutoGameSoftwareSync`).
2. Run it. A short guided tutorial explains each step: choose your shared folder, add your first game, adjust the settings.
3. Repeat on your other PC with the **same shared folder** and use **Join** on the existing game.

If your shared folder is Google Drive, set it to *Mirror files* (or make the folder *available offline*), otherwise reads can be very slow.

## Linux / SteamOS

A Linux build (x86_64) is available too: SteamOS (Steam Deck, Legion Go S...), Bazzite and Ubuntu. Download `AutoGameSoftwareSync-v1.3.3-linux-x86_64.tar.gz` from the [Releases](https://github.com/Epikou/AutoGameSoftwareSync/releases/latest) page, extract it and run `install-linux.sh` (no administrator rights needed). Saves are fully compatible with the Windows version through the same shared folder. Installation guide (in French): [docs/LINUX.md](docs/LINUX.md).

**Not tested on a real SteamOS yet** (only on Ubuntu so far): feedback is very welcome, please open an [issue](https://github.com/Epikou/AutoGameSoftwareSync/issues).

## Windows SmartScreen warning

The program is not code-signed yet. At the first launch Windows may show "Windows protected your PC". Click **More info**, then **Run anyway**. This warning disappears as more people use the program.

## Safety

Make a manual copy of your saves before your first use. The program is provided as is, without warranty (see [TERMS.md](TERMS.md)).

## Known limitation

The program cannot restore a save *before* a game starts when it is launched outside a launcher integration. A Playnite extension is planned for that.

## About this project

AutoGameSoftwareSync is **vibe-coded**: I vibe, therefore I code. Passionate about games and computers for over 40 years <3

It is built with passion rather than by a big team, so expect the occasional rough edge (and the occasional bug that makes me facepalm). Any help, suggestion, bug report or idea is warmly welcome: open an [issue](https://github.com/Epikou/AutoGameSoftwareSync/issues). Thank you for being here!

## Support

Bugs and ideas: open an [issue](https://github.com/Epikou/AutoGameSoftwareSync/issues). If you like the project, you can [buy us a coffee](https://ko-fi.com/autogamesoftwaresave).
