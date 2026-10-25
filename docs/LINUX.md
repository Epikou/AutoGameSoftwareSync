# AutoGameSoftwareSync sous Linux, SteamOS et Bazzite

AutoGameSoftwareSync synchronise vos sauvegardes et réglages de jeux et de logiciels entre plusieurs appareils grâce à
un dossier partagé (Google Drive, Dropbox, Nextcloud, Syncthing, NAS...). La version Linux fait la même chose que la
version Windows : fenêtre, mode arrière-plan, ligne de commande, détection automatique, mêmes sauvegardes.

Une sauvegarde faite sous Linux se restaure sous Windows et inversement : le dossier partagé a exactement le même format
(chemins relatifs avec `/`, emplacements « universels » comme `%APPDATA%/Jeu`, traduits vers le préfixe Proton du jeu).

> État : version de test. Compilée et essayée sous Ubuntu (WSL2 / WSLg, X11). **Pas encore essayée sur un vrai Steam
> Deck ni en mode jeu** : voir « Ce qui a été testé » en fin de page.

Deux programmes sont fournis :

| Fichier | Zone de notification | Fermer la fenêtre | Pour qui |
|---|---|---|---|
| `AutoGameSoftwareSync` | non | **quitte** l'application | tout le monde (aucune dépendance) |
| `AutoGameSoftwareSync-tray` | oui (si le bureau en a une) | réduit dans la zone, la surveillance continue | KDE Plasma (SteamOS, Bazzite), GNOME avec l'extension AppIndicator |

La variante `-tray` demande GTK 3 et `libayatana-appindicator3` (présents sur SteamOS et Bazzite en mode bureau).
Sans zone de notification, elle se comporte comme la version simple.

## 1. Installer

### SteamOS (Steam Deck, Legion Go S...) et Bazzite

1. Passez en **mode bureau** (bouton Steam → Alimentation → Passer en mode bureau).
2. Téléchargez le dossier du programme (`AutoGameSoftwareSync`, `AutoGameSoftwareSync-tray`, `install-linux.sh`,
   `icon.svg`, `icon.png`) dans par exemple `~/Téléchargements/AutoGameSoftwareSync/`.
3. Ouvrez **Konsole** et tapez :

   ```bash
   cd ~/Téléchargements/AutoGameSoftwareSync
   chmod +x install-linux.sh AutoGameSoftwareSync AutoGameSoftwareSync-tray
   ./install-linux.sh --tray --autostart
   ```

   `--tray` installe la variante avec icône (recommandé en mode bureau KDE), `--autostart` la lance à l'ouverture de la
   session de bureau (réglable ensuite dans **Réglages → Démarrage**).
4. Lancez **AutoGameSoftwareSync** depuis le menu des applications (catégorie *Utilitaires*).

Le script n'a besoin d'aucun droit administrateur et ne touche pas au système (une mise à jour de SteamOS ne casse
rien) : programme dans `~/.local/bin`, raccourci dans `~/.local/share/applications`, icônes dans le thème `hicolor`.
Pour tout retirer : `./install-linux.sh --uninstall` (vos données restent en place).

### Ubuntu, Debian, Fedora, Arch...

Même procédure (`./install-linux.sh`, avec `--tray` si votre bureau a une zone de notification). Les boîtes
« Choisir un dossier / un fichier » passent par le portail du bureau : installez `xdg-desktop-portal` et son
interface (`xdg-desktop-portal-kde` ou `-gnome`, `-gtk`) sur les installations minimales.

## 2. Premier réglage

Comme sous Windows : **Réglages → Dossier partagé**, choisissez le dossier synchronisé par votre outil, puis
**Ajouter une synchro** pour chaque jeu ou logiciel. Dans « Choisis ton programme ou son raccourci », vous pouvez
glisser ou choisir :

* un **raccourci `.desktop`** du menu (jeu Steam, application Flatpak, raccourci créé par Wine, Heroic ou Lutris) ;
* une **AppImage**, un programme Linux, un **script** (`start.sh`...) ;
* le **`.exe`** d'un jeu Windows lancé par Proton ou Wine.

Le nom et l'icône viennent du raccourci du menu quand il existe. Les emplacements de sauvegarde sont cherchés dans le
préfixe Proton / Wine du jeu, puis, pour un jeu natif, dans `~/.local/share`, `~/.config`, `~/Documents`, `~/.<jeu>`,
`~/.config/unity3d/<studio>/<jeu>`, `~/.local/share/godot/app_userdata/<jeu>` et `~/.var/app/<id>/` (Flatpak).

Les données de l'application (configuration, journal, copies de sécurité) sont dans
`~/.local/share/AutoGameSoftwareSync` (ou `$XDG_DATA_HOME/AutoGameSoftwareSync`). La variable
`AUTOGAMESOFTWARESYNC_DATA_DIR` permet d'en choisir un autre.

**Nom de l'appareil** : c'est le nom d'hôte. Tous les Steam Deck s'appellent `steamdeck` par défaut : un court suffixe
stable lui est ajouté automatiquement (ex. `steamdeck-3f2a`) pour que deux appareils ne se confondent jamais. Vous
pouvez aussi renommer l'appareil (Paramètres système → Nom d'hôte) ou définir `AUTOGAMESOFTWARESYNC_MACHINE`.

## 3. Jeux Windows (Proton, Wine, Heroic, Lutris, Bottles)

Les sauvegardes d'un jeu Windows lancé par Proton sont dans un « faux Windows » propre au jeu :
`<bibliothèque Steam>/steamapps/compatdata/<AppID>/pfx/drive_c/users/steamuser/` (`AppData/Local`,
`AppData/Roaming`, `AppData/LocalLow`, `Documents`, `Saved Games`). L'application :

* traduit les emplacements Windows connus (`%APPDATA%`...) vers le bon préfixe ;
* repère les préfixes de Proton (toutes bibliothèques, cartes SD `/run/media/...` comprises), de Heroic
  (`~/Games/Heroic/Prefixes`), de Lutris (`~/Games/*`), de Bottles (Flatpak ou natif) et `~/.wine` ;
* enregistre la sauvegarde avec son emplacement « universel » : sur votre PC Windows, le même jeu retrouve tout seul
  `AppData\Roaming\...`, et inversement un PC Windows indique au Steam Deck où chercher dans le préfixe.

## 4. Détection automatique

Comme sous Windows, un jeu ou un logiciel connu qui démarre est proposé (« Synchronise-le ! ») : notification du
bureau 15 s après le lancement, carte à la fermeture. Sont reconnus : les jeux natifs, les `.exe` lancés par
Proton / Wine (avec le préfixe du processus), les scripts, les AppImage, les applications Flatpak, les jeux installés par
Steam, Heroic (`~/Games/Heroic`), Lutris (`~/Games`), Bottles, GOG, et les émulateurs courants (RetroArch, Dolphin,
PCSX2, RPCS3, Cemu, Ryujinx / yuzu et dérivés, DuckStation, PPSSPP, melonDS, xemu, Vita3K, shadPS4, DOSBox...).

Ne sont jamais proposés : le bureau et ses services (Plasma, GNOME, portails, PipeWire...), les coquilles et
interpréteurs seuls, Steam et ses composants (steamwebhelper, reaper, pressure-vessel, environnements d'exécution),
Proton / Wine eux-mêmes (wineserver, services Windows du préfixe), les lanceurs (Heroic, Lutris, Bottles), les
navigateurs, et l'application elle-même.

La carte **Ajouter une synchro → Jeux détectés (Steam / Heroic)** liste les jeux installés par Steam et Heroic.

## 5. Mode jeu (Steam Deck) : ajouter l'application comme « jeu non-Steam »

1. En **mode bureau**, ouvrez **Steam → Jeux → Ajouter un jeu non-Steam à ma bibliothèque…** ;
   **AutoGameSoftwareSync** apparaît dans la liste (raccourci installé par le script). Cochez-le, puis
   **Ajouter les programmes sélectionnés**.
2. En **mode jeu**, l'application est dans **Bibliothèque → Non-Steam**.
3. Manette : Steam Input la transforme en clavier et souris. Dans les réglages de manette de cette entrée, choisissez
   le modèle **« Clavier (WASD) et souris »** ou **« Gamepad avec joystick de souris »**. Raccourcis utiles à associer :
   `Tab` / `Maj+Tab` (élément suivant / précédent), `Entrée` (valider), `Échap` (fermer), `Ctrl+Tab` /
   `Ctrl+Maj+Tab` (onglet suivant / précédent ; par exemple sur R1 / L1). L'écran tactile et les pavés tactiles
   fonctionnent comme une souris.
4. Sur Steam Deck et en mode jeu, le **mode grand format** (« Auto », réglage par défaut) agrandit textes et boutons.

En mode jeu, l'application est une fenêtre plein écran comme un jeu : quittez-la avec le bouton Steam → Quitter le
jeu, ou **Réglages → Configuration → Quitter**. Le démarrage automatique ne vaut qu'en mode bureau.

## 6. Démarrage automatique et arrière-plan

* **Réglages → Démarrage → « Ouvrir AutoGameSoftwareSync à l'ouverture de la session de bureau »** (ou
  `./install-linux.sh --autostart`) crée `~/.config/autostart/AutoGameSoftwareSync.desktop`. Avec l'icône de zone de
  notification, l'application démarre dans la zone ; sans elle, la fenêtre s'ouvre **réduite** dans la barre des
  tâches.
* **Service utilisateur systemd** (sans fenêtre, pour une machine relais ; facultatif) :
  `~/.config/systemd/user/autogamesoftwaresync.service` :

  ```ini
  [Unit]
  Description=AutoGameSoftwareSync (surveillance)

  [Service]
  ExecStart=%h/.local/bin/AutoGameSoftwareSync watch --quiet
  Restart=on-failure

  [Install]
  WantedBy=default.target
  ```

  puis `systemctl --user enable --now autogamesoftwaresync.service`. Une seule surveillance doit tourner à la fois
  (n'ouvrez pas la fenêtre en même temps que ce service).

## 7. Dossier partagé : outils de synchro et dossiers montés

* **Syncthing, Nextcloud, Dropbox** (dossier local synchronisé) : les changements sont vus immédiatement (`inotify`).
* **Dossiers montés** (Google Drive via rclone ou KIO, Nextcloud en WebDAV, NAS en SMB / NFS / SSHFS, carte SD,
  disque USB dans `/run/media/...`) : les fichiers déposés par l'autre appareil ne déclenchent souvent aucun
  événement ; l'application interroge alors directement le dossier partagé (toutes les 2 s en « Instantané », réglable
  dans **Réglages → Surveillance → Réactivité**). Le dossier partagé doit être monté avant le démarrage (l'application
  l'attend jusqu'à 5 min).
* Comme sous Windows, un outil cloud en mode « fichiers à la demande » ralentit tout : gardez le dossier disponible
  hors ligne.
* Noms de fichiers : les chemins avec espaces, accents et caractères non latins sont gérés. Évitez dans les sauvegardes
  les noms interdits par Windows (`: * ? " < > |`) et deux fichiers ne différant que par la casse (`Save.dat` et
  `save.dat`) si vous synchronisez aussi avec un PC Windows.
* Les liens symboliques ne sont pas suivis (seuls les vrais fichiers sont sauvegardés). Un fichier ouvert en écriture
  (verrou `flock`) reporte la restauration, comme un fichier verrouillé sous Windows.

## 8. Notifications

Elles passent par le service de notifications du bureau (D-Bus) : KDE Plasma sur SteamOS / Bazzite, GNOME, etc. Un
clic sur la notification ouvre la fenêtre. Sans service de notifications (mode jeu de SteamOS, session minimale),
elles sont simplement ignorées (une ligne dans le journal) ; la carte de proposition s'affiche quand même dans la
fenêtre. Test : `AutoGameSoftwareSync notify-test`.

## 9. Ligne de commande

Tout ce que fait la version Windows existe : `AutoGameSoftwareSync game add`, `backup`, `restore`, `versions`,
`watch`, `detect`... (voir `AutoGameSoftwareSync --help`). Le fichier `status.json` est écrit dans le dossier de
données, avec le même contenu que sous Windows.

## 10. Limites connues

* **Manette lue par Steam Input** : l'application ne lit pas la manette directement (pas de réglage « Manette » sous
  Linux) ; hors Steam, utilisez le clavier, la souris ou l'écran tactile.
* **Règles de démarrage avancées** de la version Windows (lancer un outil cloud avant l'application, délais) :
  absentes ; un seul interrupteur de démarrage.
* **Plein écran** : l'application ne sait pas qu'un jeu est en plein écran (aucune interface commune à X11 et Wayland) ;
  elle ne passe au premier plan qu'à la fermeture du jeu, et sous Wayland le bureau peut refuser (demande d'attention
  à la place).
* Icônes : PNG du thème du bureau et vignettes de la bibliothèque Steam ; les icônes SVG seules et l'icône intégrée
  d'un `.exe` ou d'une AppImage ne sont pas lues (icône générique à la place).
* Jeux Linux natifs : la base intégrée ne connaît que les emplacements Windows ; pour un jeu natif, l'emplacement est
  deviné par son nom (voir § 2) ou choisi à la main.

## 11. Ce qui a été testé

**Testé (Ubuntu 24.04 sous WSL2 / WSLg, X11)** : compilation des deux variantes ; tous les tests automatiques
(détection sur faux `/proc`, préfixes Proton / Heroic / Bottles simulés, raccourcis `.desktop`, Flatpak, AppImage,
vocabulaire des 5 langues, aller-retour réel de sauvegardes Windows ↔ Linux) ; interface en français, anglais, russe,
espagnol et chinois à 1280×800, avec et sans grand format ; ajout d'un jeu par raccourci avec nom et icône ; carte de
détection d'un jeu Steam natif ; deux « appareils » synchronisés de bout en bout par la ligne de commande sur un dossier
local (inotify) puis sur un dossier monté sans événements (interrogation directe).

**Non testé** : SteamOS / Bazzite réels, mode jeu et Steam Input, un vrai jeu sous Proton, cartes SD, affichage de
l'icône de zone de notification et des notifications (WSLg n'a ni zone de notification ni service de notifications),
KDE Plasma Wayland avec mise à l'échelle, vrais montages Google Drive / Nextcloud / SMB.

## 12. Compiler soi-même

Dépendances (Debian/Ubuntu) : `build-essential pkg-config libxkbcommon-dev libwayland-dev libx11-dev libgl1-mesa-dev
libegl1-mesa-dev` (et `libgtk-3-dev libayatana-appindicator3-dev` pour la variante `tray`). Puis :

```bash
cargo build --release                  # AutoGameSoftwareSync (sans zone de notification)
cargo build --release --features tray  # variante avec icône de zone de notification
```
