#!/bin/sh
# "Play in RyM" on Linux: rym:// links (and RyM in the app menu) open the RyM in this folder.
# Run it once from the unpacked RyM-linux folder: ./install-rym-links.sh
# It writes ~/.local/share/applications/rym.desktop and makes it the rym:// handler; nothing
# needs root. RyM Slint is used when it's here, RymPlayer-GTK otherwise.
set -e
here=$(cd "$(dirname "$0")" && pwd)
app="$here/RymPlayer-GTK"
[ -x "$here/RyM-Slint" ] && app="$here/RyM-Slint"
mkdir -p "$HOME/.local/share/applications"
cat > "$HOME/.local/share/applications/rym.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=RyM
Comment=YouTube Music player
Exec="$app" %u
Icon=$here/rym.png
Categories=AudioVideo;Audio;Player;
Terminal=false
MimeType=x-scheme-handler/rym;
EOF
command -v update-desktop-database >/dev/null && update-desktop-database "$HOME/.local/share/applications" || true
command -v xdg-mime >/dev/null && xdg-mime default rym.desktop x-scheme-handler/rym || true
echo "rym:// links now open $app"
