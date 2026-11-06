#!/bin/bash
# Packs RymPlayer-GTK (with RyM beside it for the Google sign-in) into an AppImage.
set -e
source "$HOME/.cargo/env"
cd ~
mkdir -p appimage && cd appimage
[ -x linuxdeploy ] || curl -sSLo linuxdeploy https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage
[ -x linuxdeploy-plugin-gtk.sh ] || curl -sSLo linuxdeploy-plugin-gtk.sh https://raw.githubusercontent.com/linuxdeploy/linuxdeploy-plugin-gtk/master/linuxdeploy-plugin-gtk.sh
chmod +x linuxdeploy linuxdeploy-plugin-gtk.sh
rm -rf AppDir RyM-*.AppImage
mkdir -p AppDir/usr/bin
cat > rym.desktop <<'EOF'
[Desktop Entry]
Type=Application
Name=RyM
Comment=YouTube Music player
Exec=RymPlayer-GTK
Icon=rym
Categories=AudioVideo;Audio;Player;
Terminal=false
EOF
cp ~/RyM/assets/icons/rym-gui.png rym.png
export APPIMAGE_EXTRACT_AND_RUN=1 DEPLOY_GTK_VERSION=4 NO_STRIP=1
./linuxdeploy --appdir AppDir \
  --executable ~/RyM/target/release/RymPlayer-GTK \
  --desktop-file rym.desktop --icon-file rym.png \
  --plugin gtk >/tmp/linuxdeploy.log 2>&1 || { tail -30 /tmp/linuxdeploy.log; exit 1; }
# The sign-in program beside it, on the system's GTK 3 and WebKitGTK (not bundled).
cp ~/RyM/target/release/RyM AppDir/usr/bin/RyM
LDAI_OUTPUT=RyM-x86_64.AppImage ./linuxdeploy --appdir AppDir --output appimage >>/tmp/linuxdeploy.log 2>&1 || { tail -30 /tmp/linuxdeploy.log; exit 1; }
ls -la RyM-x86_64.AppImage
du -sh AppDir
echo "== done"
