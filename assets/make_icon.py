"""Draws the app icon (a white rounded tile with the black brand circle) into icon.ico and icon.png.

Every size is drawn on its own, so the small ones (tray, title bar) stay sharp instead of being
shrunk from the big one. Run it from anywhere after changing the shapes: python make_icon.py
Needs Pillow (pip install pillow).
"""

from pathlib import Path

from PIL import Image, ImageDraw

HERE = Path(__file__).parent
SIZES = [16, 20, 24, 32, 40, 48, 64, 256]
# Each size is drawn this many times bigger, then scaled down, for smooth edges.
SCALE = 8


def draw(size: int) -> Image.Image:
    big = size * SCALE
    image = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    canvas = ImageDraw.Draw(image)

    # Bigger icons get a little room around the tile, like other Windows 11 icons.
    margin = (size // 32) * SCALE
    radius = round(size * 0.22) * SCALE
    border = max(1, round(size / 96)) * SCALE
    tile = (margin, margin, big - margin - 1, big - margin - 1)

    # A faint outline, so the white tile still has an edge on a white background.
    canvas.rounded_rectangle(tile, radius, fill=(0, 0, 0, 40))
    inner = (tile[0] + border, tile[1] + border, tile[2] - border, tile[3] - border)
    canvas.rounded_rectangle(inner, radius - border, fill=(255, 255, 255, 255))

    # The brand mark: a black circle, an even number of pixels wide so it sits exactly centered.
    diameter = max(2, round(size * 0.44 / 2) * 2) * SCALE
    start = (big - diameter) // 2
    canvas.ellipse((start, start, start + diameter - 1, start + diameter - 1), fill=(0, 0, 0, 255))

    return image.reduce(SCALE)


images = [draw(size) for size in SIZES]
images[-1].save(HERE / "icon.ico", sizes=[(size, size) for size in SIZES], append_images=images[:-1])
images[-1].save(HERE / "icon.png")
