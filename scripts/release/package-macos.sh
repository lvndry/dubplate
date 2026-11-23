#!/bin/sh
# Build and verify the files for an Apple Silicon GitHub release.
# See docs/development.md for signing credentials and the release workflow.
set -eu

usage() {
  echo "usage: $0 [--adhoc | --sign-only] [--binaries PATH] OUTPUT_DIR" >&2
  exit 1
}

fail() {
  echo "error: $*" >&2
  exit 1
}

repository=$(cd "$(dirname "$0")/../.." && pwd)
binaries=
mode=notarize
while [ "$#" -gt 1 ]; do
  case "$1" in
    --adhoc|--sign-only)
      [ "$mode" = notarize ] || usage
      mode=${1#--}
      ;;
    --binaries)
      [ "$#" -ge 3 ] || usage
      shift
      binaries=$1
      ;;
    *) usage ;;
  esac
  shift
done
[ "$#" -eq 1 ] || usage
[ "$(uname -s)-$(uname -m)" = Darwin-arm64 ] || fail "packaging requires a Mac with Apple Silicon"

if [ "$mode" != adhoc ]; then
  : "${DUBPLATE_SIGN_IDENTITY:?set DUBPLATE_SIGN_IDENTITY or pass --adhoc}"
fi
if [ "$mode" = notarize ] && [ -z "${DUBPLATE_NOTARY_PROFILE:-}" ]; then
  : "${DUBPLATE_NOTARY_KEY_PATH:?set DUBPLATE_NOTARY_PROFILE or DUBPLATE_NOTARY_KEY_PATH}"
  : "${DUBPLATE_NOTARY_KEY_ID:?set DUBPLATE_NOTARY_KEY_ID}"
  : "${DUBPLATE_NOTARY_ISSUER_ID:?set DUBPLATE_NOTARY_ISSUER_ID}"
fi

mkdir -p "$1"
output=$(cd "$1" && pwd)
if [ -z "$binaries" ]; then
  binaries="$repository/target/release"
  (cd "$repository" && cargo build --release --locked -p dubplate -p dubplate-desktop --target-dir target)
fi

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$repository/apps/dubplate-desktop/Cargo.toml")
cli_version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$repository/apps/dubplate-cli/Cargo.toml")
printf '%s\n' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$' || fail "invalid Cargo version: $version"
[ "$version" = "$cli_version" ] || fail "the app and CLI versions differ"

work=$(mktemp -d)
mountpoint="$work/mount"
mounted=no
cleanup() {
  if [ "$mounted" = yes ]; then
    hdiutil detach "$mountpoint" > /dev/null || return
  fi
  rm -rf "$work"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

for executable in "$binaries/dubplate" "$binaries/dubplate-desktop"; do
  [ "$(lipo -archs "$executable")" = arm64 ] || fail "$executable must be arm64"
  dependencies=$(otool -L "$executable")
  printf '%s\n' "$dependencies" | awk '
    NR > 1 && $1 !~ /^\/(System\/Library|usr\/lib)\// {
      print "error: non-system library " $1
      failed = 1
    }
    END { exit failed }
  ' || fail "$executable needs a library outside macOS"
done

mkdir -p "$work/image" "$work/mount"
cp "$binaries/dubplate-desktop" "$work/dubplate-desktop"
"$repository/scripts/bundle-macos.sh" --binary "$work/dubplate-desktop"
mv "$work/Dubplate.app" "$work/image/"
app="$work/image/Dubplate.app"
app_resources="$app/Contents/Resources"
cp "$repository/LICENSE" "$app_resources/"
cp "$repository/apps/dubplate-desktop/assets/fonts/"*-LICENSE.txt "$app_resources/"
cp "$repository/apps/dubplate-desktop/assets/sampler/CC0-1.0.txt" "$app_resources/"

archive="dubplate-$version-aarch64-apple-darwin"
cli="$work/$archive/bin/dubplate"
mkdir -p "$(dirname "$cli")"
cp "$binaries/dubplate" "$cli"
cp "$repository/LICENSE" "$repository/README.md" "$work/$archive/"

minimum_macos() {
  xcrun vtool -show-build "$1" | awk '/^[[:space:]]*(minos|version)[[:space:]]+[0-9]/ { print $2; exit }'
}

app_minimum=$(minimum_macos "$app/Contents/MacOS/Dubplate")
cli_minimum=$(minimum_macos "$cli")
[ -n "$app_minimum" ] && [ -n "$cli_minimum" ] || fail "could not determine the macOS deployment targets"
plutil -replace LSMinimumSystemVersion -string "$app_minimum" "$app/Contents/Info.plist"

submit_for_notarization() {
  submission=$1
  if [ -n "${DUBPLATE_NOTARY_PROFILE:-}" ]; then
    set -- --keychain-profile "$DUBPLATE_NOTARY_PROFILE"
  else
    set -- --key "$DUBPLATE_NOTARY_KEY_PATH" --key-id "$DUBPLATE_NOTARY_KEY_ID" --issuer "$DUBPLATE_NOTARY_ISSUER_ID"
  fi
  echo "Submitting $(basename "$submission") for notarization"
  result="$work/notary-result.json"
  if ! xcrun notarytool submit "$submission" "$@" --wait --timeout 25m --output-format json > "$result"; then
    cat "$result" >&2
    fail "notarytool could not finish the submission"
  fi
  status=$(plutil -extract status raw -o - "$result")
  if [ "$status" != Accepted ]; then
    cat "$result" >&2
    identifier=$(plutil -extract id raw -o - "$result")
    xcrun notarytool log "$identifier" "$@" >&2 || true
    fail "Apple returned '$status' for $(basename "$submission")"
  fi
}

if [ "$mode" = adhoc ]; then
  codesign --force --sign - "$app"
  codesign --force --sign - "$cli"
else
  codesign --force --sign "$DUBPLATE_SIGN_IDENTITY" --options runtime --timestamp "$app"
  codesign --force --sign "$DUBPLATE_SIGN_IDENTITY" --options runtime --timestamp --identifier com.lvndry.dubplate.cli "$cli"
fi
if [ "$mode" = notarize ]; then
  # Submit both executables. The app can carry a ticket, and the CLI is verified online.
  mkdir "$work/notary"
  ditto "$app" "$work/notary/Dubplate.app"
  cp "$cli" "$work/notary/dubplate"
  ditto -c -k "$work/notary" "$work/Dubplate.zip"
  submit_for_notarization "$work/Dubplate.zip"
  xcrun stapler staple "$app"
fi

codesign --verify --strict "$cli"
[ "$("$cli" --version)" = "dubplate $version" ] || fail "the CLI does not report the release version"

dmg="$work/Dubplate-$version.dmg"
ln -s /Applications "$work/image/Applications"
hdiutil create -quiet -volname "Dubplate $version" -fs HFS+ -format UDZO -srcfolder "$work/image" "$dmg"
if [ "$mode" != adhoc ]; then
  codesign --sign "$DUBPLATE_SIGN_IDENTITY" --timestamp "$dmg"
  codesign --verify --strict "$dmg"
fi
if [ "$mode" = notarize ]; then
  submit_for_notarization "$dmg"
  xcrun stapler staple "$dmg"
  codesign --verify --strict "$dmg"
  xcrun stapler validate "$dmg"
  spctl --assess --type open --context context:primary-signature --verbose "$dmg"
fi
hdiutil verify -quiet "$dmg"
hdiutil attach "$dmg" -readonly -nobrowse -mountpoint "$mountpoint" > /dev/null
mounted=yes
codesign --verify --deep --strict "$mountpoint/Dubplate.app"
if [ "$mode" = notarize ]; then
  xcrun stapler validate "$mountpoint/Dubplate.app"
  spctl --assess --type execute --verbose "$mountpoint/Dubplate.app"
fi
hdiutil detach "$mountpoint" > /dev/null
mounted=no

tar -czf "$work/$archive.tar.gz" -C "$work" "$archive"
mkdir "$work/unpacked"
tar -xzf "$work/$archive.tar.gz" -C "$work/unpacked"
codesign --verify --strict "$work/unpacked/$archive/bin/dubplate"
"$work/unpacked/$archive/bin/dubplate" --help > /dev/null

cp "$dmg" "$output/"
cp "$dmg" "$output/Dubplate.dmg"
cp "$work/$archive.tar.gz" "$repository/scripts/install.sh" "$output/"
(
  cd "$output"
  shasum -a 256 "Dubplate-$version.dmg" Dubplate.dmg "$archive.tar.gz" install.sh > SHA256SUMS
  shasum -a 256 -c SHA256SUMS
)
echo "Built Dubplate $version for Apple Silicon (app: macOS $app_minimum or later, CLI: macOS $cli_minimum or later) in $output"
