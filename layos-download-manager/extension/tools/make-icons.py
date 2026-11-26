"""Draws the Layó's Download Manager icon: a down arrow over a bar cut into sections.

Run: python3 tools/make-icons.py  (needs Pillow). Writes src/icons/icon-{16,32,48,128}.png
and ../app-icon.png (1024 px) for the desktop app.
"""
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
# Colours from layothedeveloper.com: cobalt accent, deeper cobalt, lime spark.
TOP, BOTTOM = (0x3a, 0x7a, 0xff), (0x1b, 0x47, 0xd3)
LIME = (0xbe, 0xf0, 0x50, 255)


def draw(size: int) -> Image.Image:
    s = 4 * size  # draw big, then shrink for smooth edges
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    # Vertical gradient inside a rounded square.
    grad = Image.new("RGBA", (s, s))
    g = ImageDraw.Draw(grad)
    for y in range(s):
        t = y / (s - 1)
        g.line([(0, y), (s, y)], fill=tuple(round(a + (b - a) * t) for a, b in zip(TOP, BOTTOM)) + (255,))
    mask = Image.new("L", (s, s), 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, s - 1, s - 1], radius=s * 0.22, fill=255)
    img.paste(grad, (0, 0), mask)

    d = ImageDraw.Draw(img)
    white = (255, 255, 255, 255)
    # Arrow: shaft and head.
    cx = s / 2
    shaft_w = s * 0.13
    d.rectangle([cx - shaft_w / 2, s * 0.17, cx + shaft_w / 2, s * 0.47], fill=white)
    d.polygon([(cx - s * 0.25, s * 0.42), (cx + s * 0.25, s * 0.42), (cx, s * 0.66)], fill=white)
    # Sections bar: three pieces with gaps.
    top, bottom = s * 0.73, s * 0.83
    left, right = s * 0.18, s * 0.82
    gap = s * 0.045
    width = (right - left - 2 * gap) / 3
    for i in range(3):
        x0 = left + i * (width + gap)
        # The last section is the one still downloading: lime.
        d.rounded_rectangle([x0, top, x0 + width, bottom], radius=(bottom - top) / 2, fill=white if i < 2 else LIME)
    return img.resize((size, size), Image.LANCZOS)


if __name__ == "__main__":
    out = ROOT / "src" / "icons"
    out.mkdir(parents=True, exist_ok=True)
    for n in (16, 32, 48, 128):
        draw(n).save(out / f"icon-{n}.png")
    draw(1024).save(ROOT.parent / "app-icon.png")
    print("icons written")
