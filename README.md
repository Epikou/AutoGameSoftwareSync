# AutoGameSoftwareSync (AGSS)

**English** | [Français](README.fr.md) | [Русский](README.ru.md) | [Español](README.es.md) | [中文](README.zh.md)

### Never lose anything again. Protected, synced, everywhere.

Your game saves and software settings, protected and synced between all your devices: desktop PC, laptop, handheld (Legion Go S, Steam Deck...). Pick up your game right where you left it, find your software setup wherever you are. Everything goes through **your vault**: any shared folder your devices can all see (Google Drive, OneDrive, Dropbox, Nextcloud, Syncthing, a NAS, a USB drive...). Free for personal use, Windows and Linux.

[**Download the latest version**](https://github.com/Epikou/AutoGameSoftwareSync/releases/latest) · [Support the project on Ko-fi](https://ko-fi.com/autogamesoftwaresave)

![Home screen](screenshots/en/home.png)

## Key features

- **Everything at a glance**: one tile per game, software or folder, with its state ("Safe · 2 min ago", "Copying… 64 %", "Newer version from LEGION-GO") and where it is up to date across your devices.
- **Go back in time**: a day-by-day timeline of every version from every device. Bring yesterday's version back in one click, and **undo** if you change your mind: a safety copy is always made first.
- **Your vault is found on its own**: Google Drive, OneDrive, Dropbox, iCloud, MEGA, pCloud, Nextcloud, Syncthing, network and USB drives. A vault you already use (on another device, or from an older version) is offered first.
- **PC scan**: your games (Steam, Epic, GOG, Xbox, Ubisoft, EA, itch, emulators), your software, your personal folders and Windows shortcuts. What is safe is ticked, the rest is suggested; development tools and system programs are never proposed.
- **New PC: bring everything back**: pick your old device, check, recover. Works across user names and from Windows to Linux; a game not installed yet is recovered as soon as it appears.
- **A first launch that really sets things up**: 6 short steps (language, vault, this device, scan, anything else, first copy), then everything happens on its own.
- **Automatic, verified updates**: signed by the author, installed at a quiet moment, and the previous version comes back by itself if something goes wrong.
- **Simple by default, expert when you want**: plain words and 6 settings cards; an expert mode keeps every technical option.
- Never loses data: every copy is a new folder, older versions are kept, nothing is ever overwritten in your vault.
- Detects your games when you play them (and never your command-line tools), full gamepad navigation, big-screen mode for handhelds, dark and light themes. Languages: English, Français, Русский, Español, 中文.

| | |
|---|---|
| ![Go back in time](screenshots/en/time-travel.png) | ![First launch](screenshots/en/first-launch.png) |
| ![Item page](screenshots/en/item.png) | ![Your vault](screenshots/en/vault.png) |
| ![New PC](screenshots/en/new-pc.png) | ![Settings](screenshots/en/settings.png) |
| ![Big-screen mode](screenshots/en/big-screen.png) | ![Light theme](screenshots/en/home-light.png) |

## How to use

1. Download the archive from the [Releases](https://github.com/Epikou/AutoGameSoftwareSync/releases/latest) page, unzip it and put `AutoGameSoftwareSync.exe` in a permanent folder (for example `C:\Tools\AutoGameSoftwareSync`).
2. Run it. The first launch guides you in 6 steps: language, your vault, this device, a scan of your PC, anything else, and the first copy.
3. On your other devices, install it the same way and pick the **same vault**: your items are found and brought back.

If your vault is on Google Drive, set Drive to *Mirror files* (or make the folder *available offline*), otherwise reads can be very slow. The app tells you when it detects the on-demand mode.

## Upgrading from 1.3.x

Version 2.0.0 is the first version with automatic updates, so **install it by hand once**: close the old version and replace `AutoGameSoftwareSync.exe` with the new one, in the same folder. Your settings, your items and your vault are kept as they are; the first launch shows "What's new in 2.0" and only asks what is new. Making a manual copy of your vault before upgrading is always a good idea. From then on, updates install themselves (Settings > Updates: Automatic / Ask me / Never).

## Linux / SteamOS

A Linux build (x86_64) is available too: SteamOS (Steam Deck, Legion Go S...), Bazzite and Ubuntu. Download `AutoGameSoftwareSync-v2.0.0-linux-x86_64.tar.gz` from the [Releases](https://github.com/Epikou/AutoGameSoftwareSync/releases/latest) page, extract it and run `install-linux.sh` (no administrator rights needed). Copies are fully compatible with the Windows version through the same vault, including Windows games run with Proton. Installation guide (in French): [docs/LINUX.md](docs/LINUX.md).

![Linux](screenshots/en/linux.png)

## Windows SmartScreen warning

The program is not code-signed yet. At the first launch Windows may show "Windows protected your PC". Click **More info**, then **Run anyway**. This warning disappears as more people use the program.

## Privacy

- Your saves and settings go only to the vault you choose.
- **Optional usage statistics**, only if you click "Yes, I'll help" (asked at first launch and after an update): at most once a day, version, system, language, features used, errors by category, ranges, and the names of the known titles you protect (from the built-in database and profiles), with a random ID renewed every 13 months. Never: names you type, files, paths, device name; the IP address is not kept. Kept 90 days, hosted by Cloudflare. Turn it off, see what is sent or erase your data in **Settings > Usage data**.
- For an unknown program, the app may run an anonymous web search with its name. It also asks GitHub whether an update exists and, by default, installs it automatically after verifying the author's signature (Settings > Updates: Automatic / Ask me / Never).
- Details: [PRIVACY.md](PRIVACY.md).

## Safety

Make a manual copy of your saves before your first use. The program is provided as is, without warranty (see [TERMS.md](TERMS.md)).

## Known limitation

The program cannot bring a save back *before* a game starts when it is launched outside a launcher integration. A Playnite extension is planned for that.

## About this project

AutoGameSoftwareSync is **vibe-coded**: I vibe, therefore I code. Passionate about games and computers for over 40 years <3

It is built with passion rather than by a big team, so expect the occasional rough edge (and the occasional bug that makes me facepalm). Any help, suggestion, bug report or idea is warmly welcome: open an [issue](https://github.com/Epikou/AutoGameSoftwareSync/issues). Thank you for being here!

## Support

Bugs and ideas: open an [issue](https://github.com/Epikou/AutoGameSoftwareSync/issues). If you like the project, you can [buy us a coffee](https://ko-fi.com/autogamesoftwaresave).
