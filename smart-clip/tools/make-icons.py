"""Draws the Smart Clip icon: a white clipboard on a cobalt tile, with a lime spark.

Run: python3 tools/make-icons.py  (needs Pillow). Writes the desktop app's icons in
app/src-tauri/icons/, the floating text-box icon, and app-icon.png (1024 px).
"""
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
# Colours from layothedeveloper.com: cobalt accent, deeper cobalt, lime spark.
TOP, BOTTOM = (0x3a, 0x7a, 0xff), (0x1b, 0x47, 0xd3)
LIME = (0xbe, 0xf0, 0x50, 255)
WHITE = (255, 255, 255, 255)


def spark(d: ImageDraw.ImageDraw, cx: float, cy: float, r: float, fill) -> None:
    """A four-pointed spark."""
    k = r * 0.28
    d.polygon([(cx, cy - r), (cx + k, cy - k), (cx + r, cy), (cx + k, cy + k),
               (cx, cy + r), (cx - k, cy + k), (cx - r, cy), (cx - k, cy - k)], fill=fill)


def draw(size: int) -> Image.Image:
    s = 4 * size  # draw big, then shrink for smooth edges
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    grad = Image.new("RGBA", (s, s))
    g = ImageDraw.Draw(grad)
    for y in range(s):
        t = y / (s - 1)
        g.line([(0, y), (s, y)], fill=tuple(round(a + (b - a) * t) for a, b in zip(TOP, BOTTOM)) + (255,))
    mask = Image.new("L", (s, s), 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, s - 1, s - 1], radius=s * 0.22, fill=255)
    img.paste(grad, (0, 0), mask)

    d = ImageDraw.Draw(img)
    mid = tuple(round(a + (b - a) * 0.5) for a, b in zip(TOP, BOTTOM)) + (255,)
    # Clipboard board: a white sheet with a cobalt inside, wider than tall-ish so it reads as a clipboard.
    x0, y0, x1, y1 = s * 0.20, s * 0.25, s * 0.70, s * 0.80
    line = s * 0.06
    d.rounded_rectangle([x0, y0, x1, y1], radius=s * 0.075, fill=WHITE)
    d.rounded_rectangle([x0 + line, y0 + line, x1 - line, y1 - line], radius=s * 0.035, fill=mid)
    # The clip at the top: a tab that overlaps the board, with a hole.
    cx = (x0 + x1) / 2
    d.rounded_rectangle([cx - s * 0.13, y0 - s * 0.06, cx + s * 0.13, y0 + s * 0.085], radius=s * 0.045, fill=WHITE)
    r = s * 0.028
    d.ellipse([cx - r, y0 - s * 0.03 - r * 0.2, cx + r, y0 - s * 0.03 + r * 1.8], fill=mid)
    # Three text lines on the sheet.
    for i, w in enumerate((0.28, 0.28, 0.17)):
        yy = s * (0.43 + i * 0.105)
        d.rounded_rectangle([x0 + s * 0.11, yy, x0 + s * (0.11 + w), yy + s * 0.045], radius=s * 0.012, fill=WHITE)
    # The lime spark, ringed in cobalt so it stands off the board: the clip just arrived from another PC.
    spark(d, s * 0.73, s * 0.72, s * 0.215, mid)
    spark(d, s * 0.73, s * 0.72, s * 0.165, LIME)
    return img.resize((size, size), Image.LANCZOS)


if __name__ == "__main__":
    app = ROOT / "app" / "src-tauri" / "icons"
    app.mkdir(parents=True, exist_ok=True)
    for name, n in (("32x32.png", 32), ("128x128.png", 128), ("128x128@2x.png", 256), ("icon.png", 512)):
        draw(n).save(app / name)
    draw(256).save(app / "icon.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
    # The floating icon beside text boxes is drawn from this one, scaled to the screen's DPI.
    draw(96).save(app / "float.png")
    draw(1024).save(ROOT / "app-icon.png")
    ui = ROOT / "app" / "ui" / "brand"
    ui.mkdir(parents=True, exist_ok=True)
    draw(128).save(ui / "app-icon.png")
    print("icons written")
