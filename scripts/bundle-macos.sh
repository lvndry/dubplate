#!/bin/sh
# Packages the desktop app as Dubplate.app, with the One groove icon drawn from
# assets/logo/app-icon.svg, so macOS shows the name Dubplate and the icon in the menu bar
# and the Dock.
#
#   ./scripts/bundle-macos.sh && open target/release/Dubplate.app
#
# Options:
#   --skip-build     package the binary already in target/release
#   --binary PATH    package PATH instead, next to it, without building (implies --skip-build)
#   --link           hard link the binary instead of copying it, keep an icon already made and
#                    skip signing: what `cargo run` does through scripts/run-macos.sh
#
# The release bundle gets an ad hoc signature, which lets it run on the Mac that built it.

set -eu

repository=$(cd "$(dirname "$0")/.." && pwd)
binary="$repository/target/release/dubplate-desktop"
icon_artwork="$repository/assets/logo/app-icon.svg"
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$repository/apps/dubplate-desktop/Cargo.toml" | head -n 1)
build=yes
link=no

while [ $# -gt 0 ]; do
  case "$1" in
    --skip-build) build=no ;;
    --binary)
      binary=$2
      build=no
      shift
      ;;
    --link) link=yes ;;
    *)
      echo "error: unknown option $1 (expected --skip-build, --binary PATH or --link)" >&2
      exit 1
      ;;
  esac
  shift
done

if [ "$(uname -s)" != "Darwin" ]; then
  echo "error: Dubplate.app can only be built on macOS" >&2
  exit 1
fi

if [ "$build" = yes ]; then
  cargo build --release --locked -p dubplate-desktop --manifest-path "$repository/Cargo.toml"
fi

bundle="$(dirname "$binary")/Dubplate.app"
resources="$bundle/Contents/Resources"
icon="$resources/AppIcon.icns"
if [ "$link" = no ]; then
  rm -rf "$bundle"
fi
mkdir -p "$bundle/Contents/MacOS" "$resources"
if [ "$link" = yes ]; then
  ln -f "$binary" "$bundle/Contents/MacOS/Dubplate"
else
  cp "$binary" "$bundle/Contents/MacOS/Dubplate"
fi

if [ ! -f "$icon" ] || [ "$icon_artwork" -nt "$icon" ]; then
  work=$(mktemp -d)
  trap 'rm -rf "$work"' EXIT
  iconset="$work/AppIcon.iconset"
  mkdir -p "$iconset"
  for size in 16 32 128 256 512; do
    double=$((size * 2))
    sips -s format png -z "$size" "$size" "$icon_artwork" --out "$iconset/icon_${size}x${size}.png" > /dev/null
    sips -s format png -z "$double" "$double" "$icon_artwork" --out "$iconset/icon_${size}x${size}@2x.png" > /dev/null
  done
  iconutil --convert icns --output "$icon" "$iconset"
fi

cat > "$bundle/Contents/Info.plist" << PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>
  <string>Dubplate</string>
  <key>CFBundleDisplayName</key>
  <string>Dubplate</string>
  <key>CFBundleIdentifier</key>
  <string>com.lvndry.dubplate</string>
  <key>CFBundleExecutable</key>
  <string>Dubplate</string>
  <key>CFBundleIconFile</key>
  <string>AppIcon</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>$version</string>
  <key>CFBundleVersion</key>
  <string>$version</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>LSMinimumSystemVersion</key>
  <string>11.0</string>
  <key>LSApplicationCategoryType</key>
  <string>public.app-category.music</string>
  <key>NSHighResolutionCapable</key>
  <true/>
  <key>NSHumanReadableCopyright</key>
  <string>GPL-3.0-or-later</string>
</dict>
</plist>
PLIST
plutil -lint "$bundle/Contents/Info.plist" > /dev/null

if [ "$link" = no ]; then
  codesign --force --sign - "$bundle"
  codesign --verify --strict "$bundle"
  echo "Built $bundle"
fi
