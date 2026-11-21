#!/bin/sh
# Builds the desktop app in release mode and packages it as Dubplate.app, with the One groove
# icon drawn from assets/logo/app-icon.svg.
#
#   ./scripts/bundle-macos.sh && open target/release/Dubplate.app
#
# Pass --skip-build to package the binary already in target/release. The app gets an ad hoc
# signature, which lets it run on the Mac that built it.

set -eu

repository=$(cd "$(dirname "$0")/.." && pwd)
bundle="$repository/target/release/Dubplate.app"
binary="$repository/target/release/dubplate-desktop"
icon_artwork="$repository/assets/logo/app-icon.svg"
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$repository/apps/dubplate-desktop/Cargo.toml" | head -n 1)
build=yes

for argument in "$@"; do
  case "$argument" in
    --skip-build) build=no ;;
    *)
      echo "error: unknown option $argument (expected --skip-build)" >&2
      exit 1
      ;;
  esac
done

if [ "$(uname -s)" != "Darwin" ]; then
  echo "error: Dubplate.app can only be built on macOS" >&2
  exit 1
fi

if [ "$build" = yes ]; then
  cargo build --release --locked -p dubplate-desktop --manifest-path "$repository/Cargo.toml"
fi

rm -rf "$bundle"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp "$binary" "$bundle/Contents/MacOS/Dubplate"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
iconset="$work/AppIcon.iconset"
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
  sips -s format png -z "$size" "$size" "$icon_artwork" --out "$iconset/icon_${size}x${size}.png" > /dev/null
  double=$((size * 2))
  sips -s format png -z "$double" "$double" "$icon_artwork" --out "$iconset/icon_${size}x${size}@2x.png" > /dev/null
done
iconutil --convert icns --output "$bundle/Contents/Resources/AppIcon.icns" "$iconset"

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

codesign --force --sign - "$bundle"
codesign --verify --strict "$bundle"
echo "Built $bundle"
