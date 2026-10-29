# status.json (contrat de lecture)

Fichier `<donnees AGSS>\status.json`, UTF-8 sans BOM, ecrit atomiquement par l'application (lecture seule pour les tiers).
Schema 2, additif : ignorer les champs inconnus ; tout champ absent a sa valeur par defaut.

## Document
`schema`, `app_version`, `updated_at_utc`, `watching`, `shared_folder_reachable`, `shared_folder_configured`,
`machine` (nom de ce PC), `machine_color`, `games[]`, et :
- `devices[]` : `{ "id", "name", "color" }` appareils connus (ce PC en premier), couleur `#RRGGBB`.

## games[]
- `state` : `synced` (protege) | `needs_backup` | `cloud_newer` | `conflict` | `never_synced` | `running` | `paused` | `unreachable` | `no_shared_folder` | `error` (voir `detail`).
- `last_sync_utc` : ISO 8601 UTC (`2026-10-27T08:15:00Z`), derniere synchro reussie (sauvegarde ou restauration) ; `null` si jamais.
- `origin_device` : nom AFFICHE de l'appareil qui a produit la version en place sur ce PC (ce PC si elle vient d'ici).
- `origin_device_color` : couleur `#RRGGBB` de cet appareil (carte « Mes appareils »), `null` si inconnue.
- `remote_head_machine` / `machine_color` : appareil et couleur de la derniere version du cloud.

```json
{ "name": "Mon jeu", "key": "mon-jeu", "state": "synced",
  "last_sync_utc": "2026-10-27T08:15:00Z",
  "origin_device": "PC-SALON", "origin_device_color": "#D55E00" }
```

# playnite-library.json (contrat de lecture, 2.0.3)

Fichier `<donnees AGSS>\playnite-library.json`, ecrit par l'extension Playnite 1.2.0 (ou plus recente) a cote de
`status.json` : UTF-8 sans BOM, une ligne, remplacement atomique (fichier temporaire puis renommage). L'application ne
fait que le LIRE (jamais la base LiteDB de Playnite). Contrat complet cote extension : `docs/PLAYNITE-LIBRARY.md` du
depot de l'extension ; exemple exact dans `tests/fixtures/playnite/playnite-library.sample.json`.

Schema 1, additif : l'application ignore les champs inconnus (aucun `deny_unknown_fields`) ; tout champ absent a sa
valeur par defaut (chaine vide, liste vide, `false`).

## Document
- `schema` (entier, 1 ; un format plus ancien est ignore) ;
- `generated_utc` (ISO 8601 UTC, optionnel) : une liste de **plus de 30 jours** est consideree perimee et ignoree
  (Playnite ne la met plus a jour) ; date absente ou illisible = liste valide ;
- `games[]` : un element par jeu INSTALLE.

## games[]
- `id`, `name` (nom vide = jeu ignore), `source` (« Steam », « GOG »... vide = ajoute a la main), `plugin_id` ;
- `install_dir` : ignore s'il est vide, s'il commence par `{` (variable Playnite non resolue) ou par un seul `\`
  (chemin tronque) ; un partage `\NAS\...` est accepte ;
- `exes[]` : programmes des actions « fichier » ; `game_id`, `steam_appid` (Steam) ;
- `platforms[]` : un jeu sans programme dont aucune plateforme n'est PC (Windows / Linux / Mac / SteamOS) est un jeu
  emule : jamais propose en 2.0.3 ;
- `hidden` (bool) : jeu masque dans Playnite, jamais propose ;
- `roms[]`, `emulator{name, install_dir, profile, exe}` : lus et toleres, pas exploites en 2.0.3.

Ce que l'application en fait : section « Bibliotheque Playnite » du scan du PC (jeux PC qu'aucun lanceur n'a deja vus,
pas encore proteges) et carte de l'accueil « N jeux Playnite pas encore proteges » (meme compte). Fichier absent ou
illisible pendant son remplacement : le dernier compte est garde.
