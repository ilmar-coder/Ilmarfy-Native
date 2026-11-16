#!/bin/bash
# Compila o Ilmarfy para Mac e instala /Applications/Ilmarfy.app
set -euo pipefail
cd "$(dirname "$0")/.."

APP_NAME="Ilmarfy"
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
OUT="target.nosync/$APP_NAME.app"
DEST="${INSTALL_DIR:-/Applications}"
mkdir -p "$DEST"

echo "==> Compilando (release)..."
cargo build --release -p ilmarfy-desktop

echo "==> Ícone..."
ICONSET="target.nosync/Ilmarfy.iconset"
rm -rf "$ICONSET" && mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
    sips -z $size $size crates/ilmarfy-desktop/assets/icon.png --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
    sips -z $((size * 2)) $((size * 2)) crates/ilmarfy-desktop/assets/icon.png --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o target.nosync/Ilmarfy.icns

echo "==> Montando $APP_NAME.app..."
rm -rf "$OUT"
mkdir -p "$OUT/Contents/MacOS" "$OUT/Contents/Resources"
cp target.nosync/release/ilmarfy-desktop "$OUT/Contents/MacOS/ilmarfy"
cp target.nosync/Ilmarfy.icns "$OUT/Contents/Resources/Ilmarfy.icns"
cat > "$OUT/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>$APP_NAME</string>
    <key>CFBundleDisplayName</key><string>$APP_NAME</string>
    <key>CFBundleIdentifier</key><string>br.com.ilmar.ilmarfy</string>
    <key>CFBundleVersion</key><string>$VERSION</string>
    <key>CFBundleShortVersionString</key><string>$VERSION</string>
    <key>CFBundleExecutable</key><string>ilmarfy</string>
    <key>CFBundleIconFile</key><string>Ilmarfy</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>LSMinimumSystemVersion</key><string>12.0</string>
    <key>LSApplicationCategoryType</key><string>public.app-category.music</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
codesign --force --deep --sign - "$OUT"

echo "==> Instalando em $DEST..."
rm -rf "$DEST/$APP_NAME.app"
cp -R "$OUT" "$DEST/"
echo "✅ Pronto: $DEST/$APP_NAME.app"
