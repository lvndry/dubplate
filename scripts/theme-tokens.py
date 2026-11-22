#!/usr/bin/env python3
"""Rewrites the desktop's `theme::NAME` colour constants as `theme::active().name`.

Run it on apps/dubplate-desktop/src after merging code written against the old constants:

    python3 scripts/theme-tokens.py apps/dubplate-desktop/src

Sizes, radii and the played opacity stay constants and are left as they are.
"""

import pathlib
import re
import sys

KEPT_CONSTANTS = {
    "PLAYED_OPACITY",
    "TEXT_SIZE",
    "SMALL_TEXT_SIZE",
    "TINY_TEXT_SIZE",
    "CONTROL_RADIUS",
    "SMALL_RADIUS",
    "GUTTER",
}
CONSTANT = re.compile(r"\btheme::([A-Z][A-Z_]+)\b")


def convert(match: re.Match) -> str:
    name = match.group(1)
    if name in KEPT_CONSTANTS:
        return match.group(0)
    return f"theme::active().{name.lower()}"


def main() -> None:
    root = pathlib.Path(sys.argv[1])
    for path in sorted(root.rglob("*.rs")):
        if path.name == "theme.rs":
            continue
        text = path.read_text()
        converted = CONSTANT.sub(convert, text)
        if converted != text:
            path.write_text(converted)
            print(path)


if __name__ == "__main__":
    main()
