#!/bin/bash
# Packs the Linux programs into RyM-linux-x86_64.tar.gz (no outside tools).
set -e
cd ~
rm -rf RyM-linux && mkdir RyM-linux
cp ~/RyM/target/release/RymPlayer-GTK ~/RyM/target/release/RyM RyM-linux/
strip RyM-linux/RymPlayer-GTK RyM-linux/RyM
cp ~/RyM/assets/icons/rym-gui.png RyM-linux/rym.png
cat > RyM-linux/README.txt <<'EOF'
RyM for Linux (x86_64)

RymPlayer-GTK   the window app (run this)
RyM             the terminal player; the window app also uses it to sign in to Google,
                so keep the two in the same folder

Needs GTK 4 (4.12 or newer), and WebKitGTK 4.1 for the Google sign-in window.
  Arch:           sudo pacman -S gtk4 webkit2gtk-4.1 alsa-lib pipewire-alsa
  Fedora/Nobara:  sudo dnf install gtk4 webkit2gtk4.1 alsa-lib pipewire-alsa
  Ubuntu/PikaOS:  sudo apt install libgtk-4-1 libwebkit2gtk-4.1-0 libasound2t64
Optional: yt-dlp (with Node.js), VLC.
EOF
tar czf RyM-linux-x86_64.tar.gz RyM-linux
cp RyM-linux-x86_64.tar.gz /mnt/c/Users/Willy/Desktop/
ls -la RyM-linux/ RyM-linux-x86_64.tar.gz
echo "== libraries the window app needs:"
ldd RyM-linux/RymPlayer-GTK | grep -E "gtk|glib|asound|ssl|not found" | sed 's/ (0x.*//' | sort -u
