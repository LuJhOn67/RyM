#!/bin/bash
# Packs RyM Slint (the same window app as on Windows) into RyM-Slint-x86_64.AppImage, with the
# Japanese and dot-digit fonts. GTK 3 and WebKitGTK (the tray icon's menu, the Google sign-in
# window) come from the system, as with RyM GTK's sign-in.
set -e
[ -f "$HOME/.cargo/env" ] && source "$HOME/.cargo/env"
cd ~
mkdir -p appimage && cd appimage
[ -x linuxdeploy ] || curl -sSLo linuxdeploy https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage
chmod +x linuxdeploy
rm -rf AppDirSlint RyM-Slint-*.AppImage
mkdir -p AppDirSlint/usr/bin
cat > rym-slint.desktop <<'EOF'
[Desktop Entry]
Type=Application
Name=RyM Slint
Comment=YouTube Music player
Exec=RyM-Slint
Icon=rym-slint
Categories=AudioVideo;Audio;Player;
Terminal=false
EOF
cp ~/RyM/assets/icons/rym-slint.png rym-slint.png
export APPIMAGE_EXTRACT_AND_RUN=1 NO_STRIP=1
./linuxdeploy --appdir AppDirSlint \
  --executable ~/RyM/target/release/RyM-Slint \
  --desktop-file rym-slint.desktop --icon-file rym-slint.png \
  --exclude-library 'libgtk-3*' --exclude-library 'libgdk-3*' --exclude-library 'libwebkit2gtk*' \
  --exclude-library 'libjavascriptcoregtk*' --exclude-library 'libsoup-3*' >/tmp/linuxdeploy-slint.log 2>&1 || { tail -30 /tmp/linuxdeploy-slint.log; exit 1; }
# The fonts (fontconfig is told about this folder at start).
mkdir -p ~/fonts-cache
[ -s ~/fonts-cache/NotoSansJP.ttf ] || curl -sSfL -o ~/fonts-cache/NotoSansJP.ttf "https://raw.githubusercontent.com/google/fonts/main/ofl/notosansjp/NotoSansJP%5Bwght%5D.ttf"
[ -s ~/fonts-cache/OFL.txt ] || curl -sSfL -o ~/fonts-cache/OFL.txt "https://raw.githubusercontent.com/google/fonts/main/ofl/notosansjp/OFL.txt"
mkdir -p AppDirSlint/usr/share/rym/fonts
cp ~/fonts-cache/NotoSansJP.ttf ~/fonts-cache/OFL.txt ~/RyM/ui/fonts/Doto.ttf ~/RyM/ui/fonts/OFL-doto.txt AppDirSlint/usr/share/rym/fonts/
LDAI_OUTPUT=RyM-Slint-x86_64.AppImage ./linuxdeploy --appdir AppDirSlint --output appimage >>/tmp/linuxdeploy-slint.log 2>&1 || { tail -30 /tmp/linuxdeploy-slint.log; exit 1; }
ls -la RyM-Slint-x86_64.AppImage
