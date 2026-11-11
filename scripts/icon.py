"""Draws veilq's app icon (ui/public/favicon.svg, four rounded rectangles)
as crates/veilq/assets/veilq.ico and veilq.png. Run it after changing the
favicon: `python scripts/icon.py` (needs Pillow).
"""

from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "crates" / "veilq" / "assets"
S = 1024  # drawn large, scaled down per size
U = S / 32  # one unit of the favicon's 32x32 view box


def draw() -> Image.Image:
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle((0, 0, S - 1, S - 1), radius=7 * U, fill="#3a6df0")
    for x, y, w, alpha in [(7, 9, 18, 255), (7, 15, 10, 140), (7, 21, 14, 255)]:
        bar = Image.new("RGBA", (S, S), (0, 0, 0, 0))
        ImageDraw.Draw(bar).rounded_rectangle(
            (x * U, y * U, (x + w) * U, (y + 4) * U), radius=2 * U, fill=(255, 255, 255, alpha)
        )
        img = Image.alpha_composite(img, bar)
    return img


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    img = draw()
    sizes = [16, 20, 24, 32, 40, 48, 64, 128, 256]
    img.resize((256, 256), Image.LANCZOS).save(
        OUT / "veilq.ico", sizes=[(s, s) for s in sizes]
    )
    img.resize((512, 512), Image.LANCZOS).save(OUT / "veilq.png")


main()
