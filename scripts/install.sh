#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Install Dubplate from a GitHub Release:
#   curl -fsSL https://github.com/lvndry/dubplate/releases/latest/download/install.sh | sh

set -eu

releases=https://github.com/lvndry/dubplate/releases
prefix=${DUBPLATE_INSTALL_DIR:-"$HOME/.local"}
bin_dir="$prefix/bin"
state_dir="$prefix/share/dubplate"
manifest="$state_dir/install-manifest.txt"
cli_destination="$bin_dir/dubplate"
app_destination=
temporary_dir=
mount_point=
cli_stage=
app_stage=
manifest_stage=
cli_installed=no
app_installed=no
locked=no
committed=no

fail() {
  printf 'dubplate-install: %s\n' "$*" >&2
  exit 1
}

cleanup() {
  if [ "$committed" = no ]; then
    if [ "$app_installed" = yes ]; then rm -rf "$app_destination"; fi
    if [ "$cli_installed" = yes ]; then rm -f "$cli_destination"; fi
    if [ -n "$app_stage" ] && [ -d "$app_stage/previous" ]; then
      mv "$app_stage/previous" "$app_destination" || printf 'Previous app saved at %s/previous\n' "$app_stage" >&2
    fi
    if [ -n "$cli_stage" ] && [ -f "$cli_stage/previous" ]; then
      mv "$cli_stage/previous" "$cli_destination" || printf 'Previous CLI saved at %s/previous\n' "$cli_stage" >&2
    fi
  fi
  if [ -n "$mount_point" ]; then
    hdiutil detach -quiet "$mount_point" || hdiutil detach -force -quiet "$mount_point"
  fi
  if [ -n "$temporary_dir" ]; then rm -rf "$temporary_dir"; fi
  if [ -n "$app_stage" ] && { [ "$committed" = yes ] || [ ! -e "$app_stage/previous" ]; }; then rm -rf "$app_stage"; fi
  if [ -n "$cli_stage" ] && { [ "$committed" = yes ] || [ ! -e "$cli_stage/previous" ]; }; then rm -rf "$cli_stage"; fi
  if [ -n "$manifest_stage" ]; then rm -f "$manifest_stage"; fi
  if [ "$locked" = yes ]; then rmdir "$state_dir/.install-lock"; fi
}

trap 'set +e; cleanup' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

usage() {
  cat <<'USAGE'
Install the Dubplate desktop app and command line tool on an Apple Silicon Mac.

Usage: install.sh [--app-only | --cli-only | --uninstall | --help]

The app goes in /Applications when writable, otherwise ~/Applications.
The command line tool goes in ~/.local/bin. Run the installer again to upgrade.
Use --uninstall to remove the app and CLI installed by this script.

DUBPLATE_VERSION selects a specific version instead of the latest release.
DUBPLATE_INSTALL_DIR sets the CLI and installer state prefix (default ~/.local).
DUBPLATE_APP_DIR sets the folder for Dubplate.app.
USAGE
}

want_cli=yes
want_app=yes
case "${1:-}" in
  '') ;;
  --cli-only) want_app=no ;;
  --app-only) want_cli=no ;;
  --uninstall) ;;
  -h | --help) usage; exit 0 ;;
  *) fail "unknown option $1 (see --help)" ;;
esac
[ "$#" -le 1 ] || fail "choose one option (see --help)"

case "$prefix" in
  /*) ;;
  *) fail "DUBPLATE_INSTALL_DIR must be an absolute path" ;;
esac

lock_installation() {
  mkdir -p "$state_dir"
  mkdir "$state_dir/.install-lock" 2>/dev/null || fail "could not lock $state_dir. Check for another installer running"
  locked=yes
}

read_manifest() {
  if [ -f "$manifest" ]; then
    old_app=$(sed -n 's/^app //p' "$manifest")
    if grep -Fqx cli "$manifest"; then old_cli=yes; fi
  fi
  case "$old_app" in
    '' | /*/Dubplate.app) ;;
    *) fail "invalid app path in $manifest" ;;
  esac
}

old_app=
old_cli=no
if [ "${1:-}" = --uninstall ]; then
  [ -f "$manifest" ] || fail "no installation recorded under $prefix"
  lock_installation
  read_manifest
  if [ -n "$old_app" ] && [ -e "$old_app" ]; then
    [ ! -L "$old_app" ] && [ -d "$old_app" ] || fail "unexpected app at $old_app"
    rm -rf "$old_app"
    printf 'Removed %s\n' "$old_app"
  fi
  if [ "$old_cli" = yes ]; then
    rm -f "$cli_destination"
    printf 'Removed %s\n' "$cli_destination"
  fi
  rm -f "$manifest"
  exit 0
fi

[ "$(uname -s)" = Darwin ] || fail "this release supports Apple Silicon Macs. See $releases"
if [ "$(uname -m)" != arm64 ] && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || true)" != 1 ]; then
  fail "this release supports Apple Silicon Macs. See $releases"
fi

download() {
  curl --fail --silent --show-error --location --retry 3 --output "$2" "$1"
}

version=${DUBPLATE_VERSION:-}
if [ -z "$version" ]; then
  release_url=$(curl --fail --silent --show-error --location --retry 3 --output /dev/null \
    --write-out '%{url_effective}' "$releases/latest") || fail "could not find the latest release"
  case "$release_url" in
    "$releases"/tag/v*) version=${release_url##*/v} ;;
    *) fail "could not identify the latest release at $release_url" ;;
  esac
fi
version=${version#v}
case "$version" in *[!0-9.]*) fail "invalid release version: $version" ;; esac
printf '%s\n' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$' || fail "invalid release version: $version"
base=${DUBPLATE_DOWNLOAD_BASE:-"$releases/download/v$version"}
cli_archive="dubplate-$version-aarch64-apple-darwin.tar.gz"
app_image="Dubplate-$version.dmg"
temporary_dir=$(mktemp -d "${TMPDIR:-/tmp}/dubplate-install.XXXXXXXX")
download "$base/SHA256SUMS" "$temporary_dir/SHA256SUMS" || fail "could not download the release checksums"

download_verified() {
  asset=$1
  expected=$(awk -v name="$asset" '
    { file = $2; sub(/^\*/, "", file) }
    file == name { hash = $1; count++ }
    END { if (count == 1) print tolower(hash) }
  ' "$temporary_dir/SHA256SUMS")
  [ "${#expected}" -eq 64 ] || fail "SHA256SUMS has no valid entry for $asset"
  case "$expected" in *[!0-9a-f]*) fail "invalid checksum for $asset" ;; esac
  download "$base/$asset" "$temporary_dir/$asset" || fail "could not download $asset"
  actual=$(shasum -a 256 < "$temporary_dir/$asset") || fail "could not calculate the checksum for $asset"
  [ "${actual%% *}" = "$expected" ] || fail "checksum mismatch for $asset"
  printf 'Verified %s\n' "$asset"
}

if [ "$want_cli" = yes ]; then download_verified "$cli_archive"; fi
if [ "$want_app" = yes ]; then download_verified "$app_image"; fi

if [ "$want_cli" = yes ]; then
  tar -xzf "$temporary_dir/$cli_archive" -C "$temporary_dir" || fail "could not unpack $cli_archive"
  cli_source="$temporary_dir/${cli_archive%.tar.gz}/bin/dubplate"
  [ -x "$cli_source" ] && [ ! -L "$cli_source" ] || fail "$cli_archive is missing bin/dubplate"
  [ "$("$cli_source" --version)" = "dubplate $version" ] || fail "the downloaded CLI could not report version $version"
fi

if [ "$want_app" = yes ]; then
  disk_image="$temporary_dir/disk-image"
  mkdir "$disk_image"
  hdiutil attach -quiet -readonly -nobrowse -noautoopen -mountpoint "$disk_image" "$temporary_dir/$app_image" < /dev/null \
    || fail "could not mount $app_image"
  mount_point=$disk_image
  app_source="$mount_point/Dubplate.app"
  identifier=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app_source/Contents/Info.plist") \
    || fail "$app_image is missing Dubplate.app"
  [ "$identifier" = com.lvndry.dubplate ] || fail "unexpected bundle identifier in $app_image"
fi

lock_installation
read_manifest

if [ "$want_app" = yes ]; then
  if [ -n "${DUBPLATE_APP_DIR:-}" ]; then
    app_dir=${DUBPLATE_APP_DIR%/}
  elif [ -n "$old_app" ]; then
    app_dir=${old_app%/*}
  elif [ -d /Applications ] && [ -w /Applications ]; then
    app_dir=/Applications
  else
    app_dir="$HOME/Applications"
  fi
  case "$app_dir" in
    /*) ;;
    *) fail "DUBPLATE_APP_DIR must be an absolute path" ;;
  esac
  app_destination="$app_dir/Dubplate.app"
  if [ -n "$old_app" ] && [ "$old_app" != "$app_destination" ]; then
    fail "Dubplate is installed at $old_app. Use --uninstall before changing the app folder"
  fi
  if [ -e "$app_destination" ] || [ -L "$app_destination" ]; then
    [ "$old_app" = "$app_destination" ] && [ -d "$app_destination" ] && [ ! -L "$app_destination" ] \
      || fail "$app_destination already exists. Move it aside before installing"
  fi
  mkdir -p "$app_dir"
  app_stage=$(mktemp -d "$app_dir/.dubplate-install.XXXXXXXX")
  ditto "$app_source" "$app_stage/Dubplate.app" || fail "could not stage Dubplate.app in $app_dir"
fi

if [ "$want_cli" = yes ]; then
  if [ -e "$cli_destination" ] || [ -L "$cli_destination" ]; then
    [ "$old_cli" = yes ] && [ -f "$cli_destination" ] && [ ! -L "$cli_destination" ] \
      || fail "$cli_destination already exists. Move it aside before installing"
  fi
  mkdir -p "$bin_dir"
  cli_stage=$(mktemp -d "$bin_dir/.dubplate-install.XXXXXXXX")
  cp -p "$cli_source" "$cli_stage/dubplate" || fail "could not stage the CLI in $bin_dir"
fi

manifest_stage=$(mktemp "$state_dir/.install-manifest.XXXXXXXX")
if [ "$want_cli" = yes ] || [ "$old_cli" = yes ]; then printf 'cli\n' >> "$manifest_stage"; fi
recorded_app=${app_destination:-"$old_app"}
if [ -n "$recorded_app" ]; then printf 'app %s\n' "$recorded_app" >> "$manifest_stage"; fi

if [ "$want_app" = yes ]; then
  if [ -d "$app_destination" ]; then mv "$app_destination" "$app_stage/previous"; fi
  mv "$app_stage/Dubplate.app" "$app_destination"
  app_installed=yes
fi
if [ "$want_cli" = yes ]; then
  if [ -f "$cli_destination" ]; then mv "$cli_destination" "$cli_stage/previous"; fi
  mv "$cli_stage/dubplate" "$cli_destination"
  cli_installed=yes
fi
mv -f "$manifest_stage" "$manifest"
committed=yes

if [ "$want_app" = yes ]; then printf 'Installed Dubplate %s in %s\n' "$version" "$app_destination"; fi
if [ "$want_cli" = yes ]; then
  printf 'Installed dubplate %s in %s\n' "$version" "$cli_destination"
  case ":${PATH:-}:" in
    *":$bin_dir:"*) ;;
    *) printf 'Add %s to your PATH to run dubplate by name.\n' "$bin_dir" ;;
  esac
fi
