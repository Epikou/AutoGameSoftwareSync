# AutoGameSoftwareSync sous Linux, SteamOS et Bazzite

**Ne perds plus jamais rien. Protégé, synchronisé, partout.**

AutoGameSoftwareSync protège tes sauvegardes et réglages de jeux et de logiciels, et les synchronise entre tous tes
appareils grâce à ton **coffre** : un dossier partagé que tes appareils voient tous (Nextcloud, Dropbox, Syncthing,
Google Drive, NAS, clé USB...). La version Linux fait la même chose que la version Windows : fenêtre, arrière-plan,
ligne de commande, détection automatique, scan, Remonter le temps, Nouveau PC, mêmes copies.

Une copie faite sous Linux se remet sous Windows et inversement : le coffre a exactement le même format (chemins
relatifs avec `/`, emplacements « universels » comme `%APPDATA%/Jeu`, traduits vers le préfixe Proton du jeu).

> État : version 2.0.0. Compilée et essayée sous Ubuntu (WSL2 / WSLg, X11). **Pas encore essayée sur un vrai Steam
> Deck ni en mode jeu** : voir « Ce qui a été testé » en fin de page.

Deux programmes sont fournis :

| Fichier | Zone de notification | Fermer la fenêtre | Pour qui |
|---|---|---|---|
| `AutoGameSoftwareSync` | non | **quitte** l'application | tout le monde (aucune dépendance) |
| `AutoGameSoftwareSync-tray` | oui (si le bureau en a une) | réduit dans la zone, la protection continue | KDE Plasma (SteamOS, Bazzite), GNOME avec l'extension AppIndicator |

La variante `-tray` demande GTK 3 et `libayatana-appindicator3` (présents sur SteamOS et Bazzite en mode bureau).
Sans zone de notification, elle se comporte comme la version simple.

## 1. Installer

### SteamOS (Steam Deck, Legion Go S...) et Bazzite

1. Passe en **mode bureau** (bouton Steam → Alimentation → Passer en mode bureau).
2. Télécharge `AutoGameSoftwareSync-v2.0.0-linux-x86_64.tar.gz` depuis la page des versions et extrais-le, par
   exemple dans `~/Téléchargements/AutoGameSoftwareSync/` (il contient `AutoGameSoftwareSync`,
   `AutoGameSoftwareSync-tray`, `install-linux.sh`, `icon.svg`, `icon.png`, ce guide).
3. Ouvre **Konsole** et tape :

   ```bash
   cd ~/Téléchargements/AutoGameSoftwareSync
   chmod +x install-linux.sh AutoGameSoftwareSync AutoGameSoftwareSync-tray
   ./install-linux.sh --tray --autostart
   ```

   `--tray` installe la variante avec icône (recommandé en mode bureau KDE), `--autostart` la lance à l'ouverture de la
   session de bureau (réglable ensuite dans **Réglages → Protection automatique → Démarrer avec l'ordinateur**).
4. Lance **AutoGameSoftwareSync** depuis le menu des applications (catégorie *Utilitaires*).

Le script n'a besoin d'aucun droit administrateur et ne touche pas au système (une mise à jour de SteamOS ne casse
rien) : programme dans `~/.local/bin`, raccourci dans `~/.local/share/applications`, icônes dans le thème `hicolor`.
Pour tout retirer : `./install-linux.sh --uninstall` (tes données et ton coffre restent en place).

### Ubuntu, Debian, Fedora, Arch...

Même procédure (`./install-linux.sh`, avec `--tray` si ton bureau a une zone de notification). Les boîtes
« Choisir un dossier / un programme » passent par le portail du bureau : installe `xdg-desktop-portal` et son
interface (`xdg-desktop-portal-kde` ou `-gnome`, `-gtk`) sur les installations minimales.

## 2. Premier lancement : 6 étapes qui configurent tout

1. **Bienvenue** : langue, et la question des statistiques d'usage (facultatives, « Non merci » par défaut).
2. **Ton coffre** : l'application cherche toute seule `~/Nextcloud`, `~/Dropbox`, les dossiers de Syncthing, `~/MEGA`,
   `~/pCloudDrive`, `~/OneDrive`, `~/Google Drive`, les partages réseau montés (SMB, NFS, SSHFS, WebDAV, rclone) et
   les clés USB (`/media`, `/run/media`). Un coffre **déjà utilisé** (par un autre appareil ou une version
   précédente) est proposé en premier. « Autre dossier… » pour n'importe quel dossier.
3. **Cet appareil** : son nom (le nom d'hôte) et sa couleur.
4. **Le scan** : jeux Steam (y compris les préfixes Proton), Heroic, Lutris, logiciels installés (raccourcis
   `.desktop`), dossiers perso (XDG). Ce qui est sûr est coché, le reste est proposé sans être coché.
5. **Autre chose ?** : un programme ou un dossier à ajouter toi-même (glisser-déposer ou bouton).
6. **Ta façon de faire** : démarrer avec la session, proposer les nouveaux jeux, mises à jour automatiques, puis la
   première copie.

Tu peux tout faire plus tard (« Plus tard ») et tout refaire (**Réglages → Refaire la configuration**).

**Ajouter toi-même** (page **Ajouter → Un programme ou un jeu**), tu peux glisser ou choisir :

* un **raccourci `.desktop`** du menu (jeu Steam, application Flatpak, raccourci créé par Wine, Heroic ou Lutris) ;
* une **AppImage**, un programme Linux, un **script** (`start.sh`...) ;
* le **`.exe`** d'un jeu Windows lancé par Proton ou Wine.

Le nom et l'icône viennent du raccourci du menu quand il existe. Les emplacements sont cherchés dans le préfixe Proton /
Wine du jeu, puis, pour un jeu natif, dans `~/.local/share`, `~/.config`, `~/Documents`, `~/.<jeu>`,
`~/.config/unity3d/<studio>/<jeu>`, `~/.local/share/godot/app_userdata/<jeu>` et `~/.var/app/<id>/` (Flatpak). Le
sélecteur guidé ouvre directement le dossier le plus probable, avec des raccourcis (`.config`, `.local/share`,
Documents, chaque préfixe Proton).

Les données de l'application (configuration, journal, copies de sécurité) sont dans
`~/.local/share/AutoGameSoftwareSync` (ou `$XDG_DATA_HOME/AutoGameSoftwareSync`). La variable
`AUTOGAMESOFTWARESYNC_DATA_DIR` permet d'en choisir un autre.

**Nom de l'appareil** : c'est le nom d'hôte. Tous les Steam Deck s'appellent `steamdeck` par défaut : un court suffixe
stable lui est ajouté automatiquement (ex. `steamdeck-3f2a`) pour que deux appareils ne se confondent jamais. Tu peux
aussi renommer l'appareil (Paramètres système → Nom d'hôte) **avant** la première copie, ou définir
`AUTOGAMESOFTWARESYNC_MACHINE`.

## 3. Jeux Windows (Proton, Wine, Heroic, Lutris, Bottles)

Les sauvegardes d'un jeu Windows lancé par Proton sont dans un « faux Windows » propre au jeu :
`<bibliothèque Steam>/steamapps/compatdata/<AppID>/pfx/drive_c/users/steamuser/` (`AppData/Local`,
`AppData/Roaming`, `AppData/LocalLow`, `Documents`, `Saved Games`). L'application :

* traduit les emplacements Windows connus (`%APPDATA%`...) vers le bon préfixe ;
* repère les préfixes de Proton (toutes bibliothèques, cartes SD `/run/media/...` comprises), de Heroic
  (`~/Games/Heroic/Prefixes`), de Lutris (`~/Games/*`), de Bottles (Flatpak ou natif) et `~/.wine` ;
* enregistre la copie avec son emplacement « universel » : sur ton PC Windows, le même jeu retrouve tout seul
  `AppData\Roaming\...`, et inversement un PC Windows indique au Steam Deck où chercher dans le préfixe.

## 4. Détection automatique et scan

Un jeu ou un logiciel connu qui démarre est proposé (« Synchronise-le ! ») : notification du bureau 15 s après le
lancement, carte à la fermeture. Sont reconnus : les jeux natifs, les `.exe` lancés par Proton / Wine (avec le préfixe
du processus), les scripts, les AppImage, les applications Flatpak, les jeux installés par Steam, Heroic, Lutris,
Bottles, GOG, et les émulateurs courants (RetroArch, Dolphin, PCSX2, RPCS3, Cemu, Ryujinx / yuzu et dérivés,
DuckStation, PPSSPP, melonDS, xemu, Vita3K, shadPS4, DOSBox...). Un programme lancé depuis un terminal, les outils de
développement et les commandes système ne sont jamais proposés.

Ne sont jamais proposés non plus : le bureau et ses services (Plasma, GNOME, portails, PipeWire...), Steam et ses
composants (steamwebhelper, reaper, pressure-vessel, environnements d'exécution), Proton / Wine eux-mêmes, les lanceurs
(Heroic, Lutris, Bottles), les navigateurs, et l'application elle-même.

Le **scan** (**Ajouter → Chercher sur mon PC**) passe en revue tout ça d'un coup et propose ce qui mérite d'être
protégé.

## 5. Mode jeu (Steam Deck) : ajouter l'application comme « jeu non-Steam »

1. En **mode bureau**, ouvre **Steam → Jeux → Ajouter un jeu non-Steam à ma bibliothèque…** ;
   **AutoGameSoftwareSync** apparaît dans la liste (raccourci installé par le script). Coche-le, puis
   **Ajouter les programmes sélectionnés**.
2. En **mode jeu**, l'application est dans **Bibliothèque → Non-Steam**.
3. Manette : Steam Input la transforme en clavier et souris. Dans les réglages de manette de cette entrée, choisis le
   modèle **« Clavier (WASD) et souris »** ou **« Gamepad avec joystick de souris »**. Raccourcis utiles à associer :
   `Tab` / `Maj+Tab` (élément suivant / précédent), flèches (entre les tuiles), `Entrée` (valider), `Échap` (retour),
   `Ctrl+Tab` / `Ctrl+Maj+Tab` (onglet suivant / précédent ; par exemple sur R1 / L1). L'écran tactile et les pavés
   tactiles fonctionnent comme une souris.
4. Sur Steam Deck et en mode jeu, le **grand format** (automatique) agrandit textes, tuiles et boutons.

En mode jeu, l'application est une fenêtre plein écran comme un jeu : quitte-la avec le bouton Steam → Quitter le jeu,
ou **Réglages → Mode expert → Configuration → Quitter**. Le démarrage automatique ne vaut qu'en mode bureau.

## 6. Démarrage automatique et arrière-plan

* **Réglages → Protection automatique → Démarrer avec l'ordinateur** (ou `./install-linux.sh --autostart`) crée
  `~/.config/autostart/AutoGameSoftwareSync.desktop`. Avec l'icône de zone de notification, l'application démarre dans
  la zone ; sans elle, la fenêtre s'ouvre **réduite** dans la barre des tâches.
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
  (n'ouvre pas la fenêtre en même temps que ce service).

## 7. Le coffre : outils de synchro et dossiers montés

* **Syncthing, Nextcloud, Dropbox** (dossier local synchronisé) : les changements sont vus immédiatement (`inotify`).
* **Dossiers montés** (Google Drive via rclone ou KIO, Nextcloud en WebDAV, NAS en SMB / NFS / SSHFS, carte SD,
  disque USB dans `/run/media/...`) : les fichiers déposés par l'autre appareil ne déclenchent souvent aucun
  événement ; l'application interroge alors directement le coffre (toutes les 2 s en « Instantané », réglable en mode
  expert : **Réglages → Mode expert → Surveillance → Réactivité**). Le coffre doit être monté avant le démarrage
  (l'application l'attend jusqu'à 5 min).
* **Coffre absent** (disque débranché, montage tombé) : l'accueil affiche « Ton coffre est injoignable » ; rien n'est
  recréé à sa place, les copies reprennent toutes seules dès qu'il revient.
* Un outil en mode « fichiers à la demande » ralentit tout : garde le coffre disponible hors ligne.
* Noms de fichiers : les chemins avec espaces, accents et caractères non latins sont gérés. Évite dans les données
  protégées les noms interdits par Windows (`: * ? " < > |`) et deux fichiers ne différant que par la casse
  (`Save.dat` et `save.dat`) si tu synchronises aussi avec un PC Windows.
* Les liens symboliques ne sont pas suivis (seuls les vrais fichiers sont copiés). Un fichier ouvert en écriture
  (verrou `flock`) reporte la remise d'une version, comme un fichier verrouillé sous Windows.

## 8. Remonter le temps et Nouveau PC

* **Remonter le temps** (fiche d'un élément) : une frise par jour, un point par version et par appareil. « Remettre
  cette version » fait d'abord une copie de sécurité, et « Annuler » remet exactement l'état d'avant.
* **Nouveau PC** (accueil, carte « Ton coffre » des Réglages, ou étape 2 du premier lancement) : choisis l'ancien
  appareil, vérifie, récupère tout. D'un PC Windows vers le Steam Deck, les jeux Windows sont remis dans le préfixe
  Proton du jeu ; un jeu pas encore installé est mis de côté et proposé dès qu'il apparaît.

## 9. Mises à jour

Par défaut, l'application installe toute seule les nouvelles versions, après avoir vérifié leur signature et leur
empreinte, au moment calme (aucun jeu en cours). Le programme est remplacé d'un coup (`rename`), l'ancien reste à côté
(`.old`) et revient tout seul si la nouvelle version ne démarre pas. **Réglages → Mises à jour** : Automatique / Me
demander / Jamais. Si le programme est dans un dossier non modifiable (AppImage, `/usr/bin`...), l'application propose
seulement la page de téléchargement.

## 10. Notifications

Elles passent par le service de notifications du bureau (D-Bus) : KDE Plasma sur SteamOS / Bazzite, GNOME, etc. Un
clic sur la notification ouvre la fenêtre. Sans service de notifications (mode jeu de SteamOS, session minimale),
elles sont simplement ignorées (une ligne dans le journal) ; la carte de proposition s'affiche quand même dans la
fenêtre. Test : `AutoGameSoftwareSync notify-test`.

## 11. Ligne de commande

Tout ce que fait la version Windows existe : `AutoGameSoftwareSync game add`, `backup`, `restore`, `versions`,
`watch`, `detect`... (voir `AutoGameSoftwareSync --help`, version : `--version`). Le fichier `status.json` est écrit
dans le dossier de données, avec le même contenu que sous Windows.

## 12. Limites connues

* **Manette lue par Steam Input** : l'application ne lit pas la manette directement (pas de réglage « Manette » sous
  Linux) ; hors Steam, utilise le clavier, la souris ou l'écran tactile.
* **Règles de démarrage avancées** de la version Windows (lancer un outil cloud avant l'application, délais) :
  absentes ; un seul interrupteur de démarrage.
* **Plein écran** : l'application ne sait pas qu'un jeu est en plein écran (aucune interface commune à X11 et Wayland) ;
  elle ne passe au premier plan qu'à la fermeture du jeu, et sous Wayland le bureau peut refuser (demande d'attention
  à la place).
* Icônes : PNG du thème du bureau et vignettes de la bibliothèque Steam ; les icônes SVG seules et l'icône intégrée
  d'un `.exe` ou d'une AppImage ne sont pas lues (initiales à la place).
* Jeux Linux natifs : la base intégrée connaît surtout les emplacements Windows ; pour un jeu natif, l'emplacement est
  deviné par son nom (voir § 2), appris pendant que tu joues, ou choisi à la main.
* Les éléments propres à Windows (raccourcis du Bureau, menu Démarrer, fond d'écran) ne sont pas proposés sous Linux ;
  la catégorie s'appelle « Bureau » (raccourcis et lanceurs de ta session).

## 13. Ce qui a été testé

**Testé (Ubuntu 24.04 sous WSL2 / WSLg, X11)** : compilation des deux variantes ; tous les tests automatiques
(détection sur faux `/proc`, préfixes Proton / Heroic / Bottles simulés, raccourcis `.desktop`, Flatpak, AppImage,
vocabulaire des 5 langues, aller-retour réel de copies Windows ↔ Linux, migration Windows → Linux sur un PC simulé,
mise à jour automatique et retour arrière avec de vrais binaires) ; interface en français, anglais, russe, espagnol et
chinois en grand format ; premier lancement, accueil, fiche, Remonter le temps, scan, choix du coffre, sélecteur guidé,
Nouveau PC avec des données Linux temporaires.

**Non testé** : SteamOS / Bazzite réels, mode jeu et Steam Input, un vrai jeu sous Proton, cartes SD, affichage de
l'icône de zone de notification et des notifications (WSLg n'a ni zone de notification ni service de notifications),
KDE Plasma Wayland avec mise à l'échelle, vrais montages Google Drive / Nextcloud / SMB.

## 14. Compiler soi-même

Dépendances (Debian/Ubuntu) : `build-essential pkg-config libxkbcommon-dev libwayland-dev libx11-dev libgl1-mesa-dev
libegl1-mesa-dev` (et `libgtk-3-dev libayatana-appindicator3-dev` pour la variante `tray`). Puis :

```bash
cargo build --release                  # AutoGameSoftwareSync (sans zone de notification)
cargo build --release --features tray  # variante avec icône de zone de notification
```
