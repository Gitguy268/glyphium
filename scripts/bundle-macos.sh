#!/bin/sh
# Wraps target/release/glyphium in Glyphium.app (signed ad hoc) for the Homebrew cask.
#   cargo build --release -p glyphium && scripts/bundle-macos.sh [OUT_DIR]
set -eu
root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
out=${1:-"$root/target/release"}
version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/apps/desktop/Cargo.toml" | head -n 1)
app="$out/Glyphium.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$root/target/release/glyphium" "$app/Contents/MacOS/glyphium"
cp "$root/apps/desktop/assets/glyphium.icns" "$app/Contents/Resources/glyphium.icns"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Glyphium</string>
  <key>CFBundleDisplayName</key><string>Glyphium</string>
  <key>CFBundleIdentifier</key><string>io.github.gitguy268.glyphium</string>
  <key>CFBundleExecutable</key><string>glyphium</string>
  <key>CFBundleIconFile</key><string>glyphium</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.developer-tools</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSHumanReadableCopyright</key><string>Copyright (c) 2026 Lucas Van De Weerd. MIT.</string>
</dict>
</plist>
PLIST
codesign --force --sign - "$app"
echo "$app"
