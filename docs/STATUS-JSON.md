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
