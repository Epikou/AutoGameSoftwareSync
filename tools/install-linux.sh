#!/usr/bin/env bash
# Installation en mode utilisateur d'AutoGameSoftwareSync sous Linux / SteamOS (aucun droit administrateur).
#
# Usage :  ./install-linux.sh [--tray] [--autostart] [--no-menu] [--binary <chemin>] [--uninstall]
#   --tray        installe la variante avec icone dans la zone de notification (AutoGameSoftwareSync-tray)
#   --autostart   lance l'application a l'ouverture de la session de bureau (~/.config/autostart/AutoGameSoftwareSync.desktop)
#   --no-menu     n'ajoute pas d'entree dans le menu des applications
#   --binary      chemin du programme a installer (defaut : a cote de ce script, ou target/release/)
#   --uninstall   retire le programme, l'entree de menu, l'icone et le demarrage automatique
#                 (le dossier de donnees ~/.local/share/AutoGameSoftwareSync n'est JAMAIS supprime)
set -euo pipefail

NAME="AutoGameSoftwareSync"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_DIR="${HOME}/.local/bin"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
APP_DIR="$DATA/applications"
ICON_SVG_DIR="$DATA/icons/hicolor/scalable/apps"
ICON_PNG_DIR="$DATA/icons/hicolor/256x256/apps"
AUTOSTART_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/autostart"

AUTOSTART=0
MENU=1
UNINSTALL=0
TRAY=0
BINARY=""
while [ $# -gt 0 ]; do
  case "$1" in
    --tray) TRAY=1 ;;
    --autostart) AUTOSTART=1 ;;
    --no-menu) MENU=0 ;;
    --uninstall) UNINSTALL=1 ;;
    --binary) shift; BINARY="${1:-}" ;;
    -h|--help) sed -n '2,11p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "Option inconnue : $1" >&2; exit 2 ;;
  esac
  shift
done

refresh_caches() {
  command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$APP_DIR" >/dev/null 2>&1 || true
  command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t "$DATA/icons/hicolor" >/dev/null 2>&1 || true
  command -v kbuildsycoca6 >/dev/null 2>&1 && kbuildsycoca6 >/dev/null 2>&1 || true
  command -v kbuildsycoca5 >/dev/null 2>&1 && kbuildsycoca5 >/dev/null 2>&1 || true
}

if [ "$UNINSTALL" = 1 ]; then
  rm -f "$BIN_DIR/$NAME" "$APP_DIR/$NAME.desktop" "$ICON_SVG_DIR/$NAME.svg" "$ICON_PNG_DIR/$NAME.png" "$AUTOSTART_DIR/$NAME.desktop"
  refresh_caches
  echo "$NAME retire. Vos donnees ($DATA/$NAME) sont conservees."
  exit 0
fi

# --- programme (la variante -tray est installee sous le meme nom : un seul raccourci, un seul demarrage automatique)
if [ -z "$BINARY" ]; then
  if [ "$TRAY" = 1 ]; then CANDS=("$HERE/$NAME-tray" "$HERE/../target/release/$NAME-tray"); else CANDS=("$HERE/$NAME" "$HERE/../target/release/$NAME" "$HERE/../$NAME"); fi
  for c in "${CANDS[@]}"; do
    if [ -f "$c" ]; then BINARY="$c"; break; fi
  done
fi
if [ -z "$BINARY" ] || [ ! -f "$BINARY" ]; then
  echo "Programme introuvable. Placez ce script a cote du fichier $NAME (ou $NAME-tray avec --tray), ou utilisez --binary <chemin>." >&2
  exit 1
fi
mkdir -p "$BIN_DIR"
install -m 0755 "$BINARY" "$BIN_DIR/$NAME"
echo "Programme installe : $BIN_DIR/$NAME (depuis $(basename "$BINARY"))"

# --- icones (SVG pour le bureau, PNG 256 px pour Steam et les notifications)
for c in "$HERE/icon.svg" "$HERE/../media/icon.svg"; do
  if [ -f "$c" ]; then mkdir -p "$ICON_SVG_DIR"; install -m 0644 "$c" "$ICON_SVG_DIR/$NAME.svg"; break; fi
done
for c in "$HERE/icon.png" "$HERE/../media/icon_256.png"; do
  if [ -f "$c" ]; then mkdir -p "$ICON_PNG_DIR"; install -m 0644 "$c" "$ICON_PNG_DIR/$NAME.png"; break; fi
done

# --- entree de menu (son nom de fichier = identifiant de la fenetre : icone et regroupement corrects)
if [ "$MENU" = 1 ]; then
  mkdir -p "$APP_DIR"
  cat > "$APP_DIR/$NAME.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=$NAME
Comment=Sync your game and software saves and settings between your devices
Comment[fr]=Synchronise vos sauvegardes et reglages de jeux et de logiciels entre vos appareils
Exec=$BIN_DIR/$NAME
Icon=$NAME
Terminal=false
Categories=Utility;Game;
StartupWMClass=$NAME
EOF
  chmod 0644 "$APP_DIR/$NAME.desktop"
  echo "Entree de menu : $APP_DIR/$NAME.desktop"
fi

# --- demarrage automatique (optionnel ; meme fichier que l'interrupteur Reglages > Demarrage)
if [ "$AUTOSTART" = 1 ]; then
  mkdir -p "$AUTOSTART_DIR"
  cat > "$AUTOSTART_DIR/$NAME.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=$NAME
Exec=$BIN_DIR/$NAME --tray
Icon=$NAME
Terminal=false
X-GNOME-Autostart-enabled=true
StartupNotify=false
EOF
  echo "Demarrage automatique active : $AUTOSTART_DIR/$NAME.desktop"
fi

refresh_caches
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) echo "Note : $BIN_DIR n'est pas dans votre PATH ; lancez le programme depuis le menu ou avec $BIN_DIR/$NAME." ;;
esac
echo "Termine. Lancez $NAME depuis le menu des applications."
