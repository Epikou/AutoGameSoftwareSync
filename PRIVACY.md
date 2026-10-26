# AutoGameSoftwareSync (AGSS) - Privacy / Confidentialité

*Short, plain-language text, last updated for version 2.0. / Texte court en langage simple, mis à jour pour la version 2.0.*

## English

### 1. Your files stay yours
Your saves and settings are copied only to your vault: the shared folder you choose (Google Drive, OneDrive, Dropbox, Nextcloud, NAS, USB drive...). Their content is never sent to the author. What the app writes in your vault is described in section 6.

### 2. Usage statistics: only if you agree
- **Your choice.** At first launch, and again after an update, the app asks whether you want to help: **"Yes, I'll help"** or **"No thanks"** (two buttons of the same weight). You can change your mind at any time in **Settings > Usage data**. Without an answer, or if you say no, **nothing is sent** and nothing is kept for the author; saying no also empties the local queue.
- **Purpose.** Only to improve the app: which versions and features are used, where the first launch is abandoned, which errors happen, which known titles deserve a profile. No advertising, no sale, no sharing with third parties, no cross-referencing with other data.
- **Legal basis.** Your consent (GDPR art. 6(1)(a)), which you can withdraw at any time.
- **Exact list of what is sent.**
  - An **installation ID**: a random identifier (UUID v4) created on your PC, stored in the `usage-id` file, **renewed every 13 months**, never linked to your device name.
  - With each batch: the app **version** and the **system** (Windows or Linux).
  - **Events from a closed list**, each with its day (UTC) and values chosen from fixed lists only:
    - `app_start` (once a day): version, system, Windows 10/11 or SteamOS/Linux, processor type, interface language, simple or expert mode, started by hand or at sign-in, large mode yes/no;
    - `daily_state` (once a day): number of items (ranges 0, 1-5, 6-20, 21+), number of devices (1, 2, 3+), kind of shared folder (Google Drive, OneDrive, Dropbox, network, USB, other; deduced from the path, which is not sent), detection on/off, update mode, theme;
    - `known_items` (once a day): the **names of the known titles you protect**, only when they come from the built-in database, a built-in profile or the community database (for example "Hollow Knight", "OBS Studio"), with game/software and synced on several devices yes/no;
    - `item_added` (origin and category), `detection_answer` (yes / no / later, main reason, score range), `onboarding_step` and `onboarding_done` (step, action, duration range), `time_travel` (action, age range of the version), `conflict` (shown / resolved, choice), `sync_error` (category only: shared folder unreachable, file locked, disk full, permissions, other), `setting_changed` (setting name and value from a short list), `support_click` (place), and later `scan_done`, `migration`, `update` (ranges and results only).
  - The **country** (two letters) given by the hosting network, optional.
- **Never sent.** Names you type, file names, personal folder names, paths (except the generic templates of section 3), device name, user name, account identifiers, the content of your saves. **The IP address is never read nor stored** by the server code, and request logs are turned off.
- **When.** Grouped, **at most once a day**, network timeout of 5 seconds, never while a detected program is running. Events wait in the local file `usage-queue.jsonl` (200 events at most, dropped after 7 days without a successful send). **"See what is sent"** in Settings shows this queue in plain text, exactly as it will leave.
- **Retention.** Raw events are kept **90 days**, then deleted. After that only anonymous daily totals (counts without any identifier) are kept.
- **Host.** The server runs on **Cloudflare** (Workers and D1 database, Western Europe region), acting as processor under its standard data processing terms.
- **Your rights.** Access, erasure, withdrawal of consent, and complaint to your data protection authority (in France: the CNIL). The **"Erase my data"** button sends a request to delete everything sent with your installation ID (events and confirmations of locations), empties the local queue and creates a new ID. To get a copy of your data, open an [issue](https://github.com/Epikou/AutoGameSoftwareSync/issues) with the ID shown in Settings.

### 3. Community database of locations: only if you agree
- When you **accept** a location found automatically, an anonymous line is written to `knowledge-queue.jsonl` in the app's data folder: program name, publisher, executable file name, **generic path** such as `%APPDATA%/Studio/Program`, saves or settings, how it was found, app version. User names, personal folders, account identifiers and anything typed by hand are removed; a path that is not generic is never written.
- This file is sent **only with your consent** to usage statistics, with the daily batch.
- The server counts how many **distinct installations** confirmed each location. From **5 installations**, or after a manual check by the author, the location joins the public community list; the author can also refuse an entry. The link between a confirmation and your installation ID is removed after 90 days (replaced by a random token: the count stays right).
- If you agreed, the app also downloads this public list once a day (no identifier in that request) and uses it right after its built-in database and profiles. Every location is checked again on your PC before being suggested.

### 4. Anonymous web search
For an unknown program, the app may run an anonymous web search with its name, to find where it keeps its saves and settings.
- Only the program's name is sent, in a query such as "<name> save game location", to public search engines (DuckDuckGo, Brave Search, Mojeek, Startpage) and, if needed, to the Steam store and Wikipedia.
- No account, no cookie, no file, no folder path and no user name is sent. The request uses a standard browser identification.
- It happens rarely (a few searches per day at most), never while a detected program is running, and the result is kept on your PC so the same program is not searched again.
- Any location found is checked on your PC (the folder must exist and contain recent files) before being suggested. Nothing is added without your approval.

### 5. Updates
At most once a day, the app asks GitHub whether a new version exists, and downloads it from GitHub when you allow it (no personal data is sent; GitHub sees the usual connection data of any download). Settings > Updates: **Automatic** (default: the new version is downloaded, its signature and fingerprint are verified, then it replaces the program at a quiet moment and the app restarts; if the new version does not start within 60 seconds, the previous one is restored automatically), **Ask me**, or **Never** (no check at all). Every release carries a manifest signed with the author's key; the app refuses anything that does not match. If you agreed to usage statistics, the `update` event (old version, new version, result) is added to them; otherwise nothing is sent.

### 6. What the app writes in your vault (never sent to the author)
Your vault is your own storage: everything below stays there, and only your devices and the storage service you chose can read it.
- **Copies**: one folder per version, named with the date and the **device name** (for example `PC-DESK`), with a small `meta.json` file (device name, date, file list with sizes and fingerprints, and "universal" locations such as `%APPDATA%/Game` so another device finds the right place).
- **Position marks** (`.agss-devices/<device>/`): tiny files saying which version each device has (device name, item, version, date), so the home screen can show "Up to date on 2 devices". Marks of versions that were removed are deleted.
- **Software inventory** (`.agss-inventory/<device>/`): once a day, the names of the software installed on each device, its winget identifiers and the games of its launchers, used by "New PC" to tell you what to reinstall. No paths, no user names; the 3 most recent days are kept.
- **A marker** `.agss-vault` at the root of the vault, written once, so the app recognizes it.

## Français

### 1. Vos fichiers restent à vous
Vos sauvegardes et réglages sont copiés uniquement dans votre coffre : le dossier partagé que vous choisissez (Google Drive, OneDrive, Dropbox, Nextcloud, NAS, clé USB...). Leur contenu n'est jamais envoyé à l'auteur. Ce que l'appli écrit dans votre coffre est décrit à la section 6.

### 2. Statistiques d'usage : seulement avec votre accord
- **Votre choix.** Au premier lancement, puis après une mise à jour, l'appli vous demande si vous voulez aider : **« Oui, j'aide »** ou **« Non merci »** (deux boutons de même poids). Vous pouvez changer d'avis à tout moment dans **Réglages > Données d'usage**. Sans réponse, ou si vous refusez, **rien n'est envoyé** et rien n'est gardé pour l'auteur ; refuser vide aussi la file locale.
- **Finalité.** Uniquement améliorer l'appli : versions et fonctions utilisées, étape où le premier lancement est abandonné, erreurs rencontrées, titres connus qui méritent un profil. Pas de publicité, pas de vente, pas de transfert à des tiers, pas de croisement avec d'autres données.
- **Base légale.** Votre consentement (RGPD art. 6.1.a), retirable à tout moment.
- **Liste exacte de ce qui est envoyé.**
  - Un **identifiant d'installation** : un identifiant aléatoire (UUID v4) créé sur votre PC, gardé dans le fichier `usage-id`, **renouvelé tous les 13 mois**, jamais lié au nom de votre appareil.
  - Avec chaque envoi : la **version** de l'appli et le **système** (Windows ou Linux).
  - Des **événements d'une liste fermée**, chacun avec son jour (UTC) et des valeurs choisies uniquement dans des listes fixes :
    - `app_start` (1 fois par jour) : version, système, Windows 10/11 ou SteamOS/Linux, type de processeur, langue de l'interface, mode simple ou expert, lancé à la main ou à l'ouverture de session, grand format oui/non ;
    - `daily_state` (1 fois par jour) : nombre d'éléments (tranches 0, 1-5, 6-20, 21+), nombre d'appareils (1, 2, 3+), type de dossier partagé (Google Drive, OneDrive, Dropbox, réseau, USB, autre ; déduit du chemin, qui n'est pas envoyé), détection activée ou non, mode de mise à jour, thème ;
    - `known_items` (1 fois par jour) : les **noms des titres connus que vous protégez**, seulement s'ils viennent de la base intégrée, d'un profil intégré ou de la base communautaire (par exemple « Hollow Knight », « OBS Studio »), avec jeu/logiciel et synchronisé sur plusieurs appareils oui/non ;
    - `item_added` (origine et catégorie), `detection_answer` (oui / non / plus tard, raison principale, tranche de score), `onboarding_step` et `onboarding_done` (étape, action, tranche de durée), `time_travel` (action, tranche d'âge de la version), `conflict` (affiché / résolu, choix), `sync_error` (catégorie seulement : dossier partagé injoignable, fichier verrouillé, disque plein, droits, autre), `setting_changed` (nom du réglage et valeur d'une courte liste), `support_click` (endroit), et plus tard `scan_done`, `migration`, `update` (tranches et résultats seulement).
  - Le **pays** (deux lettres) indiqué par le réseau de l'hébergeur, facultatif.
- **Jamais envoyé.** Les noms que vous tapez, les noms de fichiers, les noms de dossiers perso, les chemins (sauf les gabarits génériques de la section 3), le nom de l'appareil, le nom d'utilisateur, les identifiants de compte, le contenu de vos sauvegardes. **L'adresse IP n'est jamais lue ni stockée** par le code du serveur, et les journaux de requêtes sont désactivés.
- **Quand.** Par lots, **au plus une fois par jour**, délai réseau de 5 secondes, jamais pendant l'utilisation d'un programme détecté. Les événements attendent dans le fichier local `usage-queue.jsonl` (200 événements au plus, abandonnés après 7 jours sans envoi réussi). **« Voir ce qui est envoyé »**, dans Réglages, affiche cette file en clair, exactement telle qu'elle partira.
- **Conservation.** Les événements bruts sont gardés **90 jours**, puis supprimés. Ensuite, seuls des totaux quotidiens anonymes (des comptes sans aucun identifiant) sont gardés.
- **Hébergeur.** Le serveur tourne chez **Cloudflare** (Workers et base D1, région Europe de l'Ouest), sous-traitant selon ses conditions standard de traitement des données.
- **Vos droits.** Accès, effacement, retrait du consentement et réclamation auprès de l'autorité de protection des données (en France : la CNIL). Le bouton **« Effacer mes données »** envoie une demande de suppression de tout ce qui a été envoyé avec votre identifiant d'installation (événements et confirmations d'emplacements), vide la file locale et crée un nouvel identifiant. Pour obtenir une copie de vos données, ouvrez une [issue](https://github.com/Epikou/AutoGameSoftwareSync/issues) avec l'identifiant affiché dans Réglages.

### 3. Base communautaire des emplacements : seulement avec votre accord
- Quand vous **acceptez** un emplacement trouvé automatiquement, une ligne anonyme est écrite dans `knowledge-queue.jsonl`, dans le dossier de données de l'appli : nom du programme, éditeur, nom du fichier exécutable, **chemin générique** comme `%APPDATA%/Studio/Programme`, sauvegardes ou réglages, manière dont il a été trouvé, version de l'appli. Les noms d'utilisateur, dossiers perso, identifiants de compte et tout texte tapé à la main sont retirés ; un chemin qui n'est pas générique n'est jamais écrit.
- Ce fichier n'est envoyé **qu'avec votre accord** aux statistiques d'usage, avec l'envoi quotidien.
- Le serveur compte combien d'**installations distinctes** ont confirmé chaque emplacement. À partir de **5 installations**, ou après une vérification manuelle de l'auteur, l'emplacement rejoint la liste communautaire publique ; l'auteur peut aussi refuser une entrée. Le lien entre une confirmation et votre identifiant d'installation est supprimé au bout de 90 jours (remplacé par un jeton aléatoire : le compte reste juste).
- Si vous avez accepté, l'appli télécharge aussi cette liste publique une fois par jour (aucun identifiant dans cette requête) et l'utilise juste après sa base intégrée et ses profils. Chaque emplacement est revérifié sur votre PC avant d'être proposé.

### 4. Recherche web anonyme
Pour un programme inconnu, l'app peut lancer une recherche web anonyme avec son nom, afin de trouver où il garde ses sauvegardes et ses réglages.
- Seul le nom du programme est envoyé, dans une requête du type « <nom> save game location », à des moteurs de recherche publics (DuckDuckGo, Brave Search, Mojeek, Startpage) et, si besoin, au magasin Steam et à Wikipédia.
- Aucun compte, aucun cookie, aucun fichier, aucun chemin de dossier ni nom d'utilisateur n'est envoyé. La requête utilise une identification de navigateur standard.
- C'est rare (quelques recherches par jour au plus), jamais pendant l'utilisation d'un programme détecté, et le résultat est gardé sur votre PC pour ne pas chercher deux fois le même programme.
- Tout emplacement trouvé est vérifié sur votre PC (le dossier doit exister et contenir des fichiers récents) avant d'être proposé. Rien n'est ajouté sans votre accord.

### 5. Mises à jour
Au plus une fois par jour, l'appli demande à GitHub si une nouvelle version existe et la télécharge depuis GitHub si vous le permettez (aucune donnée personnelle n'est envoyée ; GitHub voit les données de connexion habituelles de tout téléchargement). Réglages > Mises à jour : **Automatique** (par défaut : la nouvelle version est téléchargée, sa signature et son empreinte sont vérifiées, puis elle remplace le programme à un moment calme et l'appli redémarre ; si la nouvelle version ne démarre pas en 60 secondes, l'ancienne est remise en place automatiquement), **Me demander**, ou **Jamais** (aucune vérification). Chaque version publiée porte un manifeste signé avec la clé de l'auteur ; l'appli refuse tout ce qui ne correspond pas. Si vous avez accepté les statistiques d'usage, l'événement `update` (ancienne version, nouvelle version, résultat) s'y ajoute ; sinon rien n'est envoyé.

### 6. Ce que l'appli écrit dans votre coffre (jamais envoyé à l'auteur)
Votre coffre est votre propre stockage : tout ce qui suit y reste, et seuls vos appareils et le service de stockage que vous avez choisi peuvent le lire.
- **Les copies** : un dossier par version, nommé avec la date et le **nom de l'appareil** (par exemple `PC-BUREAU`), avec un petit fichier `meta.json` (nom de l'appareil, date, liste des fichiers avec tailles et empreintes, et emplacements « universels » comme `%APPDATA%/Jeu` pour qu'un autre appareil trouve le bon endroit).
- **Les marques de position** (`.agss-devices/<appareil>/`) : de tout petits fichiers qui disent quelle version a chaque appareil (nom de l'appareil, élément, version, date), pour que l'accueil affiche « À jour sur 2 appareils ». Les marques des versions supprimées sont effacées.
- **L'inventaire des logiciels** (`.agss-inventory/<appareil>/`) : une fois par jour, les noms des logiciels installés sur chaque appareil, leurs identifiants winget et les jeux de ses lanceurs, utilisés par « Nouveau PC » pour vous dire quoi réinstaller. Aucun chemin, aucun nom d'utilisateur ; les 3 jours les plus récents sont gardés.
- **Un marqueur** `.agss-vault` à la racine du coffre, écrit une fois, pour que l'appli le reconnaisse.
