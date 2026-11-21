#!/bin/sh
# cargo runs every binary through this script on macOS (see .cargo/config.toml). The desktop
# app starts from a Dubplate.app bundle next to its binary, so the menu bar and the Dock show
# the name Dubplate and the One groove icon; tests and the command line tool run as they are.

set -eu

binary=$1
shift
if [ "$(basename "$binary")" != "dubplate-desktop" ]; then
  exec "$binary" "$@"
fi
"$(dirname "$0")/bundle-macos.sh" --binary "$binary" --link
exec "$(dirname "$binary")/Dubplate.app/Contents/MacOS/Dubplate" "$@"
