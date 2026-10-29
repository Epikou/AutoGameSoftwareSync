# AutoGameSoftwareSync sur consoles portables Android (préversion 0.1.0)

**Ne perds plus jamais rien. Protégé, synchronisé, partout — y compris sur ta console.**

La version Android protège les **sauvegardes de tes émulateurs** (RetroArch, PPSSPP, DuckStation, AetherSX2 /
NetherSX2, Dolphin, Azahar / Lime3DS / Citra) et tes **dossiers perso** (photos, documents) avec le **même coffre** et
le **même moteur** que la version PC : versions immuables (`<coffre>/<clé>/saves/<date>-<appareil>/{files, meta.json}`),
jamais d'écrasement, copie de sécurité avant toute remise, empreintes SHA-256, Remonter le temps, marques de position
`.agss-devices`. Une partie commencée sur le PC continue sur la console et inversement.

Cible : consoles Android avec manette intégrée (Retroid Pocket, AYN Odin, Ayaneo Pocket…), écran paysage. Tout se
pilote à la **manette** (croix / stick = déplacer, A = valider, B = retour, X = synchroniser, Start = réglages,
L1 / R1 = défiler, clavier à l'écran pour le nom de l'appareil) et au **tactile**.

Préversion : testée dans l'émulateur Android (API 34, x86_64) et par les tests Rust ; **pas encore sur une vraie
console** (en particulier la croix « à axes » des manettes X-Input : corrigée par lecture, non reproduite). Voir « À
valider sur une vraie console » en fin de page.

---

## 1. Installer

1. Télécharge `AutoGameSoftwareSync-android-0.1.0-arm64.apk` (page des versions GitHub) et vérifie son SHA-256
   (fichier `.sha256` à côté).
2. Sur la console : copie l'APK (ou ouvre-le depuis le navigateur) et installe-le. Android demande d'autoriser
   « Installer des applis inconnues » pour l'appli qui ouvre le fichier : c'est normal pour une appli hors Play Store.
3. Lance AutoGameSoftwareSync. Le premier lancement fait tout en 5 étapes (langue, accès aux fichiers, nom et
   couleur de l'appareil, coffre, éléments à protéger).

Les mises à jour s'installent par-dessus (même clé de signature) sans rien perdre. L'appli ne se met **pas** à jour
toute seule : « Réglages > À propos > Ouvrir la page GitHub ».

Minimum : **Android 11 (API 30)**. En dessous (Android 10 et avant), le stockage est cloisonné pour une appli qui vise
Android 14 : elle ne verrait ni les émulateurs ni le coffre ; l'APK refuse donc de s'installer. Toutes les consoles
récentes sont en Android 11 ou plus.

## 2. L'accès aux fichiers (« Accès à tous les fichiers »)

Depuis Android 11, une appli ne voit rien du stockage sans l'autorisation **« Accès à tous les fichiers »**
(`MANAGE_EXTERNAL_STORAGE`). L'appli la demande à l'étape 2 et ouvre directement l'écran système ; reviens ensuite dans
l'appli (elle vérifie toute seule).

Cette autorisation est acceptable pour un APK distribué hors Play Store ; elle est nécessaire pour lire les dossiers
des émulateurs (`/storage/emulated/0/RetroArch`, `/PSP`, …) et le coffre.

**Si l'autorisation est retirée plus tard** (par toi, ou par un « nettoyeur » du constructeur) : l'appli le voit à
chaque ouverture et à chaque retour au premier plan, affiche un bandeau **« Accès aux fichiers manquant »** avec le
bouton qui rouvre l'écran système, met toutes les tuiles dans cet état, et **ne copie ni ne remet rien** tant qu'elle
manque (jamais de faux « Tout est à l'abri »). Dès qu'elle revient, la synchro repart toute seule.

**Ce qu'aucune autorisation ne permet** : lire `Android/data/<autre appli>/`. Depuis Android 11 (et plus du tout
depuis Android 13, même par le sélecteur de dossiers), ces dossiers sont privés. Les émulateurs qui y rangent leurs
sauvegardes doivent être **réglés sur un dossier visible** (voir le tableau). La version 0.1.0 ne tente ni SAF
(« sélecteur de dossiers ») ni accès de débogage : trop fragile pour le grand public, et sans effet depuis Android 13.

## 3. Le coffre : un dossier déjà synchronisé par une autre appli

Google Drive pour Android ne fournit aucun dossier synchronisé, et l'accès direct à Drive n'est pas dans cette version.
Le coffre est donc un **dossier local** qu'une autre appli garde identique à celui du PC :

| Appli | Recommandation |
|---|---|
| **Syncthing** (version communautaire « Syncthing-Fork » sur F-Droid / GitHub ; l'appli officielle Android est arrêtée) | Le plus simple et le plus fiable : pair à pair, sans compte. Partage un dossier, par exemple `/storage/emulated/0/Sync`, avec ton PC (où AGSS utilise le même dossier comme coffre). Mode « Envoyer et recevoir » des deux côtés. L'appli détecte ces dossiers (marqueur `.stfolder`) et les propose à l'étape 4. |
| **FolderSync** / **Autosync for Google Drive** / Dropsync… | Synchronise un dossier local avec le coffre qui est dans ton Drive / Dropbox / NAS. Règle une synchro **bidirectionnelle**, **instantanée ou fréquente**, et **« ne jamais supprimer »** si l'option existe. Choisis ce dossier local comme coffre dans l'appli. |
| Carte SD partagée (coffre copié à la main) | Marche aussi : l'appli propose les cartes SD (`/storage/XXXX-XXXX`). |

Conseils : le coffre doit être le **même dossier** des deux côtés (sous-dossier `AutoGameSoftwareSync` créé
automatiquement, marqueur `.agss-vault`). AGSS n'écrit que des **nouveaux** dossiers de version (jamais de
modification de fichier existant), ce qui convient très bien aux outils de synchro : aucun conflit de fichier possible,
au pire une version incomplète ignorée jusqu'à la fin de son transfert.

## 4. Émulateurs pris en charge et compatibilité PC ↔ Android

Le **même élément** (même clé dans le coffre, mêmes sous-espaces d'emplacements `files/@<id>/`) est créé des deux
côtés : sur le PC via le profil embarqué, sur la console via le même profil avec les chemins de la console
(`src/emulators.rs`, testé par `tests/v51.rs`). Les deux côtés doivent protéger les **mêmes emplacements cochés**
(les états instantanés sont décochés par défaut partout).

| Émulateur | Profil (clé du coffre) | Dossier Android attendu (`/storage/emulated/0/` ou carte SD) | Dossier PC | Compatibilité des fichiers |
|---|---|---|---|---|
| RetroArch | `retroarch` (« RetroArch saves ») | `RetroArch/saves`, `RetroArch/states` | `<RetroArch>\saves`, `\states` | **Sauvegardes (.srm) identiques.** États instantanés : même cœur, même version des deux côtés seulement. Garde les mêmes réglages « classer les sauvegardes par dossier » des deux côtés. |
| PPSSPP | `ppsspp` (« PPSSPP saves ») *(nouveau profil PC)* | `PSP/SAVEDATA`, `PSP/PPSSPP_STATE` (aussi `PPSSPP/PSP`) | `Documents\PPSSPP\PSP\SAVEDATA`, `\PPSSPP_STATE` | **Données de sauvegarde identiques** (même carte mémoire virtuelle) ; états instantanés : liés à la version de PPSSPP, à confirmer. Versions récentes sur Android 11+ : Réglages > Système > **Dossier de la Memory Stick** → choisir un dossier visible, par ex. `/storage/emulated/0/PSP`. |
| DuckStation | `duckstation` (« DuckStation memory cards ») | `duckstation/memcards`, `duckstation/savestates` | `Documents\DuckStation\memcards` | **Cartes mémoire (.mcd) identiques.** États instantanés liés à la version. |
| AetherSX2 / NetherSX2 (PS2) | `pcsx2` (« PCSX2 memory cards ») | `AetherSX2/memcards` ou `NetherSX2/memcards` (+ `sstates`) | `Documents\PCSX2\memcards` | **Partiel** : cartes 8 Mo `.ps2` = même format que PCSX2 ; cartes « dossier » et états instantanés non échangeables. Par défaut l'appli range tout dans `Android/data` (invisible) : Système > **Dossier de données** → dossier visible nommé `AetherSX2` ou `NetherSX2` à la racine. |
| Dolphin (GC / Wii) | `dolphin` (« Dolphin saves ») *(nouveau profil PC)* | `dolphin-emu/GC`, `dolphin-emu/Wii/title` (aussi `mmjr`, `dolphin-mmjr`) | `Documents\Dolphin Emulator\GC`, `\Wii\title` | **Cartes mémoire GC (.raw) et sauvegardes Wii identiques.** États non portables. Le Dolphin officiel sur Android 11+ range tout dans `Android/data` : seules les versions / forks qui utilisent `/storage/emulated/0/dolphin-emu` sont protégeables. |
| Azahar / Lime3DS / Citra (3DS) | `azahar` (« Azahar saves (3DS) ») *(nouveau profil PC)* | `azahar/sdmc/Nintendo 3DS`, `azahar/states` (aussi `lime3ds`, `citra-emu`) | `%APPDATA%\Azahar\sdmc\Nintendo 3DS` (ou Lime3DS / Citra) | **Partiel, à vérifier** : même arborescence, mais les noms de dossiers dépendent de l'identité de console (id0 / id1) ; copier une fois le dossier utilisateur d'un côté à l'autre pour avoir les mêmes identifiants. Au premier lancement de l'appli Android, choisir un dossier utilisateur visible nommé `azahar` à la racine. |

**Non pris en charge en 0.1.0** (données uniquement dans `Android/data`, sans réglage d'emplacement) : Yuzu et
équivalents Switch (Sudachi, Eden, Citron…), Ryujinx, melonDS, Flycast, Vita3K. Ils seront ajoutés le jour où un
dossier visible est possible, ou avec un accès SAF fiable. Les **jeux Android natifs** (données privées) ne sont pas
concernés : hors sujet de l'appli.

Détection : l'appli regarde si le dossier de l'émulateur existe **et** contient au moins un des emplacements ; un
dossier vide laissé par une désinstallation n'est pas proposé. La liste complète, avec ce qui est trouvé sur la
console, est dans **Réglages > Émulateurs**.

## 5. Ce qui est automatique (et ce qui ne l'est pas)

Android arrête les applis en arrière-plan ; il n'y a **aucun service en fond** dans cette version.

- **Automatique** : copie vers le coffre de tout ce qui a changé **à l'ouverture de l'appli** et **à chaque retour au
  premier plan** après au moins 20 s d'absence (donc après une partie si tu repasses par l'appli), et après le premier
  lancement. Un bouton **Arrêter** est proposé tant que rien n'a été écrit dans le coffre.
- **Manuel** : bouton **Synchroniser maintenant** (ou bouton **X** de la manette) ; **Protéger maintenant** dans la
  fiche d'un élément.
- **Jamais automatique sur la console : la remise d'une version**. Si le coffre a une version plus récente faite par un
  autre appareil, la tuile passe en « Version plus récente de {PC} : ouvre pour la remettre » (comptée « à regarder »)
  et la fiche propose **« Remettre la version de {PC} »** avec confirmation (copie de sécurité locale avant, Annuler
  60 s). Raison : la console ne sait pas si l'émulateur tourne encore en arrière-plan (bouton Accueil pendant une
  partie) ; une remise sous ses pieds serait écrasée à sa prochaine sauvegarde. **Ferme l'émulateur avant de valider.**
  Même règle pour un conflit (deux appareils ont modifié en même temps) : les deux versions sont gardées et la fiche
  demande toujours (« Garder la mienne » / « Prendre celle de l'autre appareil ») ; il n'y a pas de résolution
  automatique « la plus récente » sur la console (voir aussi l'horloge, § 8).
- **Pas automatique** : rien ne se passe pendant que tu joues ni quand l'appli est fermée. Habitude conseillée :
  ouvrir AGSS **avant** de lancer l'émulateur (et valider la remise s'il y en a une) et **après** avoir quitté (envoie
  la tienne). La synchronisation réelle vers le PC dépend ensuite de ton appli de synchro (Syncthing est instantané).

Même moteur que le PC pour chaque élément : point de synchro, copie, décision par chaîne de versions (jamais par
l'heure), copie de sécurité vérifiée avant toute remise.

## 6. Écrans

- **Premier lancement** : langue (en, fr, ru, es, zh), accès aux fichiers, nom + couleur de l'appareil (choix rapides,
  champ texte, et **clavier à l'écran** pilotable à la manette : les claviers Android ne tapent pas toujours dans une
  appli native ; mêmes marqueurs `.agss-devices` et mêmes couleurs que le PC), coffre (dossiers Syncthing, coffres
  existants et dossiers `AutoGameSoftwareSync` trouvés — recherche en arrière-plan avec « Un instant… », ou Parcourir),
  éléments (émulateurs trouvés avec leurs emplacements, dossiers perso — rien de coché en dehors des sauvegardes
  d'émulateurs).
- **Accueil** : tuiles avec état (À l'abri, Copie faite, Version plus récente reçue de…, Version plus récente de… :
  ouvre pour la remettre, Deux appareils ont modifié en même temps, Problème, Dossier introuvable, Dossier vide
  maintenant, Accès aux fichiers manquant), dernière copie et appareil, titre d'état global, bandeaux « Accès aux
  fichiers manquant » et « heure fausse », Synchroniser maintenant / Arrêter, Ajouter, Réglages. B sur l'accueil
  masque l'appli.
- **Fiche** : Protéger maintenant, Ne plus protéger (rien n'est supprimé), cadre « Remettre la version de {appareil} »
  quand une version plus récente attend, cadre de conflit, **Remonter le temps** : toutes les versions du coffre (date,
  appareil, fichiers, taille, étiquettes « ce que cet appareil a », « la plus récente », « gardée lors d'un conflit »,
  « copie de sécurité ») et les copies de sécurité locales ; **Remettre cette version** (confirmation, copie de
  sécurité avant) puis **Annuler** dans la barre du bas (60 s).
- **Réglages** : langue, appareil, coffre, conflits (toujours demandés sur la console), ce qui se fait tout seul,
  Ajouter, Émulateurs (tableau de compatibilité, dossiers attendus sur chaque stockage, trouvé / pas trouvé, notes
  traduites), Journal, À propos (version, page GitHub, licences), Masquer l'appli.

## 7. Construire l'APK (développeur)

Prérequis (déjà en place dans `D:\Projets\Outils\_android`) : JDK 17, SDK Android 34 (build-tools 34.0.0,
platform-tools, émulateur), NDK 27.2, `env.ps1` ; Rust avec les cibles `aarch64-linux-android` et
`x86_64-linux-android` ; `cargo install cargo-apk` (0.10). Clé de signature release dans
`D:\Projets\Outils\_android-signing` (voir son `README.txt` : **à sauvegarder hors du PC**, sans elle plus de mise à
jour par-dessus).

```powershell
cd D:\Projets\Outils\SaveSync
.\tools\build-android.ps1          # arm64-v8a signé -> D:\Projets\Outils\_dist\android\ (+ .sha256)
.\tools\build-android.ps1 -X86     # + x86_64 (émulateur de test)
.\tools\build-android.ps1 -Debug   # APK de débogage (clé de débogage)
```

À la main : `. D:\Projets\Outils\_android\env.ps1` puis, dans `android\`, `cargo apk build --release --target
aarch64-linux-android` (CARGO_TARGET_DIR conseillé : `D:\Projets\Outils\_build-android`), puis `apksigner sign` avec la
clé release (schéma v3 ; v2 n'est plus ajouté par apksigner dès que `minSdk` ≥ 30). Note : cargo-apk exige une clé pour
`--release` ; le script lui donne la clé de **débogage**
standard du SDK (mot de passe public « android ») par les variables `CARGO_APK_RELEASE_KEYSTORE` /
`CARGO_APK_RELEASE_KEYSTORE_PASSWORD` (aucun chemin local dans `android\Cargo.toml`), puis re-signe avec la vraie clé et
**vérifie que le signataire final est bien `CN=AutoGameSoftwareSync, O=Epikou`** (un APK resté signé « debug » est
refusé). La vraie clé et son mot de passe ne sont jamais dans le dépôt. Le dossier `android\` (avec `vendor\winit` et
`VENDOR-NOTE.md`) est publiable tel quel : aucun secret, aucun chemin local, `android/target` ignoré par git ; winit
modifié est cité dans `NOTICE` (Apache-2.0, deux fichiers marqués).

`versionCode` de l'APK : calculé par cargo-apk 0.10 depuis la version du paquet,
`0x01000000 + majeur × 65536 + mineur × 256 + correctif` (0.1.0 → 16777472, 0.2.0 → 16777728, 1.0.0 → 16842752).
Tout changement d'outil de construction devra rester au-dessus de ces valeurs (sinon Android refuse la mise à jour).

Organisation : `android/` est une crate séparée (`agss-android`, cdylib, paquet `io.github.epikou.agss`) qui dépend de
la bibliothèque `agss` de la racine (moteur, coffre, profils, textes partagés) avec **eframe 0.31** comme le PC
(fonction `android-native-activity`). Modules : `app.rs` (boucle, manette via `agss::padfocus`, thème sombre accent
#028463, polices + chinois système, garde-fous : autorisation retirée, horloge fausse, retour au premier plan),
`screens.rs` (écrans, clavier à l'écran), `model.rs` (préférences `android.json`, `settings.json` et `config.json` au
format PC, synchro d'un élément sans remise automatique, nettoyage au démarrage, candidats de coffre en largeur
d'abord), `worker.rs` (fil de travail : synchros, remises, et toutes les lectures du stockage), `sys.rs` (JNI :
permission, stockages, modèle, langue, page web), `i18n.rs` (textes 5 langues, notes des émulateurs, libellés,
erreurs du moteur, pluriels), `pad.rs` (logique pure des axes de la manette, miroir testé du patch winit).
`vendor/winit` : copie locale de winit 0.30.13 avec deux changements pour la manette (voir `VENDOR-NOTE.md`). Dans la
bibliothèque : `src/emulators.rs` (table PC ↔ Android), `src/rfd_shim.rs` (pas de boîtes de dialogue système sur
Android), profils PC `ppsspp`, `dolphin`, `azahar`, `backup::cleanup_claimed` (note `tmp/claimed.txt` du dossier de
version en cours d'écriture, nettoyage prudent au démarrage, PC et Android).

Tests : `cargo test` à la racine (Windows et Linux ; `tests/v51.rs` = correspondance des profils et aller-retour du
coffre PC ↔ console simulés ; `tests/v52.rs` = nettoyage d'une version incomplète après arrêt brutal) et `cargo test`
dans `android\` (préférences, candidats de coffre et budget, propositions, synchro, version disponible non remise,
conflits jamais résolus tout seuls, précédence des états, perte d'autorisation, orphelins, axes de la manette, textes et
pluriels).

Essai dans l'émulateur Android : `emulator -avd agss_test`, `adb install -r <apk x86_64>`, puis
`adb shell mkdir -p /sdcard/RetroArch/saves /sdcard/Sync/.stfolder` etc. pour simuler des émulateurs et un dossier
Syncthing ; `adb shell appops set io.github.epikou.agss MANAGE_EXTERNAL_STORAGE allow` pour accorder la permission
sans passer par l'écran système ; `adb shell input keyevent KEYCODE_BUTTON_A` (A), `KEYCODE_DPAD_*`, `KEYCODE_BUTTON_X`
(synchro) pour piloter comme une manette ; `adb exec-out screencap -p > capture.png`.

## 8. Limites connues de la 0.1.0

- Aucune synchro en arrière-plan ni au lancement d'un émulateur (voir § 5).
- `Android/data` inaccessible : émulateurs à régler sur un dossier visible ; Switch / melonDS / Flycast non pris en
  charge.
- Coffre = dossier local synchronisé par une autre appli ; pas de Google Drive direct.
- Pas de détection « l'émulateur tourne » : c'est pourquoi **aucune version n'est remise automatiquement** sur la
  console (§ 5) ; une remise manuelle pendant une partie n'est pas bloquée non plus (ferme l'émulateur avant de valider).
- **Arrêt brutal pendant une copie** (appli tuée par Android, batterie) : au démarrage suivant, l'appli retire ses
  dossiers de travail locaux (`tmp/`) et, si une version de **cet appareil** est restée sans `meta.json` dans le coffre
  (notée dans `tmp/claimed.txt` pendant l'écriture), ce dossier incomplet. Jamais une version complète, jamais celle
  d'un autre appareil (elle est peut-être en cours de transfert par la synchro). Un toast le signale. Aucune perte :
  rien n'avait été écrit comme terminé.
- **Horloge** : la console n'a pas d'accès réseau ici (pas de correction NTP comme sur le PC). Les dates des versions
  faites sur la console viennent de son horloge ; aucune décision de synchro n'en dépend (chaîne de versions), et les
  conflits sont toujours demandés. Si l'heure est antérieure à la construction de l'appli (pile à plat, date repartie
  en arrière), un bandeau « heure fausse » le dit sur l'accueil ; l'ordre de « Remonter le temps » peut alors être
  faux pour ces versions.
- Le `versionCode` de l'APK est calculé par cargo-apk depuis la version (0.1.0 → 16777472, formule au § 7) et croît avec
  chaque version ; il ne vaut pas 1.
- Clavier : le nom de l'appareil se saisit par les choix rapides (modèle, Console, Retroid…), le **clavier à l'écran**
  (manette ou tactile : lettres, chiffres, espace, effacer) ou le clavier Android s'il tape dans l'appli (non garanti
  dans une appli native : voir § 9).
- Traductions ru / es / zh automatiques, non relues (y compris les notes des émulateurs).
- Pas d'image Android 10 ou plus ancienne : l'APK exige Android 11.

## 9. À valider sur une vraie console

- Manette intégrée : croix en mode X-Input (**axes « chapeau »**) et stick gauche → flèches (patch winit corrigé par
  lecture : `is_synthetic: false`, non reproduit sur appareil), croix en touches `DPAD`, A / B / X, L1 / R1, bouton
  Retour du système. Si la croix ne répond pas : la tactile reste possible, et dire quelle console / quel mode.
- Clavier tactile Android (Gboard, clavier constructeur) dans le champ du nom ; le clavier à l'écran est le repli sûr.
- Dossiers réels des émulateurs installés (RetroArch 1.19+, PPSSPP 1.17+, DuckStation, NetherSX2, Dolphin MMJR,
  Azahar) et la compatibilité annoncée dans le tableau, en jouant vraiment des deux côtés (PPSSPP : états instantanés
  à confirmer d'une version à l'autre).
- Syncthing-Fork : délai réel entre une copie sur la console et sa visibilité sur le PC (et inversement).
- Densité d'écran et taille des boutons sur un écran 16:9 de 4 à 7 pouces.
- Carte SD : détection des émulateurs qui y sont installés, choix du coffre sur la carte.
- Écran système « Accès à tous les fichiers » sur les surcouches constructeur (Retroid, Ayaneo) ; retrait puis retour de
  l'autorisation (bandeau et reprise automatique, vérifiés dans l'émulateur).
- Retour au premier plan après une partie : synchro relancée (détection par le focus de la fenêtre, et à défaut par
  20 s sans image).
