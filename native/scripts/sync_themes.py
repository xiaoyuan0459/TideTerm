#!/usr/bin/env python3
"""Sync Qt shell theme data from the TypeScript theme definitions.

Single source of truth: native/themes/*.ts (restored from the legacy webview
frontend at git 0199a08^ -- the visual baseline for the UI realign). The
generated file maps the full web theme token set onto the Qt shell palette and
the 16-colour terminal palette.

Regenerate with:
    python native/scripts/sync_themes.py > native/src/rust/shared/theme_db/mod.rs
"""
from __future__ import annotations

import pathlib
import re

ROOT = pathlib.Path(__file__).resolve().parents[2]
THEME_DIR = ROOT / "native" / "themes"

# Optional shared palette constants (`palette.ts`). Only the retired Oceanic theme
# used them; the file is gone, so treat it as absent rather than requiring it.
PALETTE: dict[str, str] = {}
_palette_path = THEME_DIR / "palette.ts"
if _palette_path.exists():
    for obj, body in re.findall(
        r"export const (\w+) = \{([^}]+)\} as const;",
        _palette_path.read_text(encoding="utf-8"),
        re.S,
    ):
        for key, val in re.findall(r"(\w+):\s*'([^']*)'", body):
            PALETTE[f"{obj}.{key}"] = val


def resolve(value: str) -> str:
    value = value.strip()
    return PALETTE.get(value, value)


def parse_color(value: str) -> str:
    value = value.strip()
    if match := re.match(r"#([0-9a-fA-F]{6})([0-9a-fA-F]{2})?$", value):
        return f"#{match.group(1).lower()}{(match.group(2) or '').lower() or 'ff'}"
    if match := re.match(
        r"rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*,\s*([0-9.]+)\s*\)", value
    ):
        alpha = round(float(match.group(4)) * 255)
        return "#{:02x}{:02x}{:02x}{:02x}".format(
            int(match.group(1)), int(match.group(2)), int(match.group(3)), alpha
        )
    raise ValueError(f"cannot parse color: {value!r}")


VALUE_RE = re.compile(r"(\w+):\s*('[^']*'|[A-Za-z_]\w*(?:\.[A-Za-z_]\w*)*)")


def parse_object(body: str) -> dict[str, str]:
    return {
        key: resolve(val.strip("'"))
        for key, val in VALUE_RE.findall(body)
    }


def spec_for(path: pathlib.Path) -> str:
    text = path.read_text(encoding="utf-8")
    match = re.search(r"export const (\w+): Theme = \{(.*?)\n\};", text, re.S)
    if not match:
        raise ValueError(f"no Theme export in {path}")
    body = match.group(2)
    theme_id = re.search(r"id:\s*'([^']+)'", body).group(1)
    name = re.search(r"name:\s*'([^']+)'", body).group(1)
    colors = parse_object(body.split("colors: {", 1)[1].split("},", 1)[0])
    xterm = parse_object(body.split("xterm: {", 1)[1].split("}", 1)[0])

    # index 0 = default background, index 7 = default foreground in
    # tiderm-terminal's `set_base16`.
    base16 = [
        xterm["background"], xterm["red"], xterm["green"], xterm["yellow"],
        xterm["blue"], xterm["magenta"], xterm["cyan"], xterm["foreground"],
        xterm["brightBlack"], xterm["brightRed"], xterm["brightGreen"],
        xterm["brightYellow"], xterm["brightBlue"], xterm["brightMagenta"],
        xterm["brightCyan"], xterm["brightWhite"],
    ]
    base16_str = '", "'.join(parse_color(c) for c in base16)

    shell_fields = {
        "bg0": "bg",
        "bg1": "mantle",
        "bg2": "surface",
        "bgHover": "surface2",
        "bd2": "border",
        "tx1": "text",
        "tx2": "subtext",
        "tx3": "faint",
        "ac": "accent",
        "err": "danger",
    }
    # Extended per-theme tokens: legacy webview `colors.*` set. Carried for data
    # completeness only — the Qt QSS builder consumes the shell token set above, and
    # nothing in native reads these fields today.
    tide_fields = {
        "bg3": "bg_elevated",
        "bgInput": "bg_input",
        "bgTabActive": "bg_tab_active",
        "bd1": "border1",
        "bd3": "border3",
        "tx4": "text_disabled",
        "txOnAccent": "text_on_accent",
        "overlayBgStrong": "overlay_strong",
        "overlayShadow": "shadow",
        "shadowElevated": "shadow_elevated",
        "acHover": "accent_hover",
        "acSoft": "accent_soft",
        "ai": "ai",
        "aiSoft": "ai_soft",
        "ok": "ok",
        "warn": "warn",
        "info": "info",
        "statusRunning": "status_running",
        "statusConnected": "status_connected",
        "statusDisconnected": "status_disconnected",
    }
    lines = ["    ThemeSpec {"]
    lines.append(f'        id: "{theme_id}", name: "{name}",')
    for source, target in shell_fields.items():
        lines.append(f'        {target}: "{parse_color(colors[source])}",')
    for source, target in tide_fields.items():
        lines.append(f'        {target}: "{parse_color(colors[source])}",')
    lines.append(f'        scrim: "{parse_color(colors["overlayBg"])}",')
    lines.append(f'        base16: ["{base16_str}"],')
    lines.append("    },")
    return "\n".join(lines)


# Dropdown order: the branded default first, then the light theme. Ids not listed
# here still appear, sorted by file name, after the listed ones.
ORDER = ["windterm", "light"]


def order_key(path: pathlib.Path) -> tuple[int, str]:
    theme_id = re.search(r"id:\s*'([^']+)'", path.read_text(encoding="utf-8")).group(1)
    rank = ORDER.index(theme_id) if theme_id in ORDER else len(ORDER)
    return (rank, path.name)


def main() -> None:
    specs = [
        spec_for(path)
        for path in sorted(THEME_DIR.glob("*.ts"), key=order_key)
        if path.name not in {"index.ts", "palette.ts"}
    ]
    print(
        "//! Generated from `native/themes/*.ts` by `native/scripts/sync_themes.py`."
    )
    print("//! Do not edit by hand -- regenerate the file after changing a theme.")
    print()
    print("use crate::theme::ThemeSpec;")
    print()
    print("pub(crate) const THEMES: &[ThemeSpec] = &[")
    print("\n".join(specs))
    print("];")


if __name__ == "__main__":
    main()
