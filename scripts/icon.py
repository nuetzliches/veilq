"""Draws veilq's app icon as ui/public/favicon.svg has it (three lines of
text on blue, the middle one blacked out) into crates/veilq/assets/veilq.ico
and veilq.png, and the MSIX package's logos into packaging/msix/Assets.
Run it after changing the favicon: `python scripts/icon.py` (needs Pillow).
"""

from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "crates" / "veilq" / "assets"
MSIX = ROOT / "packaging" / "msix" / "Assets"
S = 1024  # drawn large, scaled down per size
U = S / 32  # one unit of the favicon's 32x32 view box


def draw() -> Image.Image:
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle((0, 0, S - 1, S - 1), radius=7 * U, fill="#3a6df0")
    # x, y, width, height, corner radius, colour: as the favicon's rects.
    for x, y, w, h, r, fill in [
        (7, 8, 18, 3.5, 1.75, "#ffffff"),
        (6, 14, 20, 5, 1, "#0f1115"),
        (7, 21, 13, 3.5, 1.75, "#ffffff"),
    ]:
        d.rounded_rectangle((x * U, y * U, (x + w) * U, (y + h) * U), radius=r * U, fill=fill)
    return img


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    img = draw()
    sizes = [16, 20, 24, 32, 40, 48, 64, 128, 256]
    img.resize((256, 256), Image.LANCZOS).save(
        OUT / "veilq.ico", sizes=[(s, s) for s in sizes]
    )
    img.resize((512, 512), Image.LANCZOS).save(OUT / "veilq.png")
    # The MSIX package's logos (ADR-0039): Start menu, tile, Store.
    MSIX.mkdir(parents=True, exist_ok=True)
    for name, size in [("Square44x44Logo", 44), ("Square150x150Logo", 150), ("StoreLogo", 50)]:
        img.resize((size, size), Image.LANCZOS).save(MSIX / f"{name}.png")


main()
