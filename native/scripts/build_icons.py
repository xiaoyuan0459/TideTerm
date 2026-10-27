"""Render the TideTerm desktop icon from a single geometry definition.

Usage: python native/scripts/build_icons.py
Requires Pillow. Emits tideterm-icon.svg and the png/ico/icns desktop formats
into native/assets/icons; the 1024px PNG is shared by all desktop formats.
"""

import math
from pathlib import Path

from PIL import Image, ImageColor, ImageDraw

ICON_DIR = Path(__file__).resolve().parents[1] / "assets" / "icons"
BASE = ICON_DIR / "tideterm-icon"

SIZE = 1024
SUPERSAMPLE = 4
CORNER = 190
CANVAS = "#172437"
INK = "#8bd4d6"
STROKE = 84

CHEVRON = [(320, 338), (484, 512), (320, 686)]
# 单峰而非双峰：16/24px 下双峰糊成一团，单峰仍保留浪的轮廓。
WAVE_X0, WAVE_X1, WAVE_Y, WAVE_AMP, WAVE_PERIODS, WAVE_STEPS = 536, 812, 686, 58, 1.0, 240
ICO_SIZES = [(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)]


def wave_points():
    return [
        (
            WAVE_X0 + (WAVE_X1 - WAVE_X0) * t,
            WAVE_Y - WAVE_AMP * math.sin(t * WAVE_PERIODS * 2 * math.pi),
        )
        for t in (i / WAVE_STEPS for i in range(WAVE_STEPS + 1))
    ]


def polyline_path(points):
    return " ".join(f"{x:.1f} {y:.1f}" for x, y in points)


def write_svg(wave):
    body = (
        f'<path d="M {polyline_path(CHEVRON)}" fill="none" stroke="{INK}" '
        f'stroke-width="{STROKE}" stroke-linecap="round" stroke-linejoin="round"/>\n'
        f'  <path d="M {polyline_path(wave)}" fill="none" stroke="{INK}" '
        f'stroke-width="{STROKE}" stroke-linecap="round" stroke-linejoin="round"/>'
    )
    svg = (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" '
        'role="img" aria-label="TideTerm terminal icon">\n'
        f'  <rect width="1024" height="1024" rx="{CORNER}" fill="{CANVAS}"/>\n'
        f"  {body}\n</svg>\n"
    )
    BASE.with_suffix(".svg").write_text(svg, encoding="utf-8", newline="\n")


def render_png():
    scale = SUPERSAMPLE
    canvas = Image.new("RGBA", (SIZE * scale, SIZE * scale), (0, 0, 0, 0))
    draw = ImageDraw.Draw(canvas)
    draw.rounded_rectangle(
        [0, 0, SIZE * scale - 1, SIZE * scale - 1], radius=CORNER * scale, fill=CANVAS
    )

    radius = STROKE * scale / 2
    ink = ImageColor.getrgb(INK)
    for points in (CHEVRON, wave_points()):
        scaled = [(x * scale, y * scale) for x, y in points]
        stamps = []
        for (x0, y0), (x1, y1) in zip(scaled, scaled[1:]):
            steps = max(1, round(math.hypot(x1 - x0, y1 - y0) / (radius / 3)))
            stamps.extend(
                (x0 + (x1 - x0) * i / steps, y0 + (y1 - y0) * i / steps)
                for i in range(steps)
            )
        stamps.append(scaled[-1])
        for x, y in stamps:
            draw.ellipse([x - radius, y - radius, x + radius, y + radius], fill=ink)

    return canvas.resize((SIZE, SIZE), Image.Resampling.LANCZOS)


def main() -> None:
    wave = wave_points()
    write_svg(wave)
    icon = render_png()
    icon.save(BASE.with_suffix(".png"), format="PNG", optimize=True)
    icon.save(BASE.with_suffix(".ico"), format="ICO", sizes=ICO_SIZES)
    icon.save(BASE.with_suffix(".icns"), format="ICNS")
    print(f"wrote {BASE}.* from {SIZE}x{SIZE} geometry")


if __name__ == "__main__":
    main()
