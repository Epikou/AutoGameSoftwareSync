# AutoGameSoftwareSync (AGSS)

[English](README.md) | **Français** | [Русский](README.ru.md) | [Español](README.es.md) | [中文](README.zh.md)

Tes sauvegardes et réglages de jeux et de logiciels, d'un PC à l'autre (fixe, console portable comme la Legion Go S...) via n'importe quel dossier partagé : Google Drive, Dropbox, un NAS, etc. Gratuit pour un usage personnel.

[**Télécharger la dernière version**](https://github.com/Epikou/AutoGameSoftwareSync/releases/latest) · [Soutenir le projet sur Ko-fi](https://ko-fi.com/autogamesoftwaresave)

## Ce que ça fait

- Sauvegarde les données d'un jeu quand vous le fermez, et restaure la version la plus récente sur votre autre PC.
- Ne perd jamais de données : chaque sauvegarde est un dossier unique, les anciennes versions sont toujours conservées, et une copie de sécurité est faite avant toute restauration.
- Détecte vos jeux (Steam, Epic, GOG) et connaît des milliers d'emplacements de sauvegarde. L'ajout manuel est aussi possible.
- Tourne discrètement près de l'horloge, peut démarrer avec Windows (y compris en mode plein écran Xbox sur console portable).
- Navigation complète à la manette (DualSense, manettes Xbox). Langues : English, Français, Русский, Español, 中文.

## Utilisation

1. Téléchargez `AutoGameSoftwareSync.exe` depuis la page [Releases](https://github.com/Epikou/AutoGameSoftwareSync/releases) et placez-le dans un dossier permanent (par exemple `C:\Tools\AutoGameSoftwareSync`).
2. Lancez-le. Un court tutoriel guidé explique chaque étape : choisir votre dossier partagé, ajouter votre premier jeu, régler les options.
3. Recommencez sur votre autre PC avec le **même dossier partagé** et utilisez **Rejoindre** sur le jeu existant.

Si votre dossier partagé est Google Drive, passez-le en *Mettre en miroir les fichiers* (ou rendez le dossier *disponible hors connexion*), sinon les lectures peuvent être très lentes.

## Linux / SteamOS

Une version Linux (x86_64) est aussi disponible : SteamOS (Steam Deck, Legion Go S...), Bazzite et Ubuntu. Téléchargez `AutoGameSoftwareSync-v1.3.3-linux-x86_64.tar.gz` depuis la page [Releases](https://github.com/Epikou/AutoGameSoftwareSync/releases/latest), extrayez-la et lancez `install-linux.sh` (aucun droit administrateur nécessaire). Les sauvegardes sont entièrement compatibles avec la version Windows via le même dossier partagé. Guide d'installation : [docs/LINUX.md](docs/LINUX.md).

**Pas encore testé sur un vrai SteamOS** (seulement sous Ubuntu pour l'instant) : les retours sont les bienvenus, ouvrez une [issue](https://github.com/Epikou/AutoGameSoftwareSync/issues).

## Alerte Windows SmartScreen

Le programme n'est pas encore signé. Au premier lancement, Windows peut afficher « Windows a protégé votre PC ». Cliquez sur **Informations complémentaires**, puis **Exécuter quand même**. Cette alerte s'estompe avec le nombre d'utilisateurs.

## Sécurité

Faites une copie manuelle de vos sauvegardes avant la première utilisation. Le programme est fourni tel quel, sans garantie (voir [TERMS.md](TERMS.md)).

## Limite connue

Le programme ne peut pas restaurer une sauvegarde *avant* le démarrage d'un jeu lancé hors d'un lanceur intégré. Une extension Playnite est prévue pour cela.

## À propos de ce projet

AutoGameSoftwareSync est **vibecodé** : je vibe donc je code. Passionné de jeux et d'informatique depuis plus de 40 ans <3

Il est fait avec passion plutôt que par une grosse équipe : attendez-vous à quelques aspérités (et à un bug de temps en temps qui me fait lever les yeux au ciel). Toute aide, suggestion, signalement de bug ou idée est la bienvenue : ouvrez une [issue](https://github.com/Epikou/AutoGameSoftwareSync/issues). Merci d'être là !

## Assistance

Bugs et idées : ouvrez une [issue](https://github.com/Epikou/AutoGameSoftwareSync/issues). Si le projet vous plaît, vous pouvez [nous offrir un café](https://ko-fi.com/autogamesoftwaresave).
