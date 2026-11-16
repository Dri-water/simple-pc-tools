"""Draws the Simple PC Tools icon: assets/app.ico, icon-256.png and icon-32.png.

    python assets/make_icon.py        (needs Pillow: pip install pillow)

The mark: a rounded blue tile (Windows 11 app-tile proportions) holding a 2 x 2
grid of white tiles, one of them a circle: a small set of simple tools. It
reads at 16 px; small sizes only enlarge the grid. Colours follow DESIGN.md
(--accent); change them there first.
"""

from pathlib import Path

from PIL import Image, ImageChops, ImageDraw

OUT = Path(__file__).resolve().parent
TOP = (77, 148, 255)      # gradient top-left, a lighter --accent
BOTTOM = (22, 92, 214)    # gradient bottom-right, --accent
WHITE = (255, 255, 255, 255)
SS = 1024                 # supersampled canvas; every size is drawn here then reduced


def tile(small: bool) -> Image.Image:
    img = Image.new("RGBA", (SS, SS), (0, 0, 0, 0))
    inset = SS * (0.02 if small else 0.06)
    radius = SS * 0.22

    # Diagonal gradient, masked to a rounded square.
    down = Image.linear_gradient("L").resize((SS, SS))                 # 0 at top -> 255 at bottom
    across = down.transpose(Image.Transpose.ROTATE_90).transpose(Image.Transpose.FLIP_LEFT_RIGHT)
    ramp = ImageChops.add(down, across, scale=2.0)                     # 0 top-left -> 255 bottom-right
    grad = Image.merge(
        "RGB",
        [ramp.point(lambda v, a=a, b=b: round(a + (b - a) * v / 255)) for a, b in zip(TOP, BOTTOM)],
    ).convert("RGBA")
    mask = Image.new("L", (SS, SS), 0)
    ImageDraw.Draw(mask).rounded_rectangle([inset, inset, SS - inset, SS - inset], radius=radius, fill=255)
    img.paste(grad, (0, 0), mask)

    # 2 x 2 grid of white tiles; the top-right one is round.
    d = ImageDraw.Draw(img)
    span = SS * (0.58 if small else 0.50)     # width of the whole grid
    gap = span * (0.16 if small else 0.14)
    cell = (span - gap) / 2
    x0 = y0 = (SS - span) / 2
    r = cell * 0.26
    for row in range(2):
        for col in range(2):
            x, y = x0 + col * (cell + gap), y0 + row * (cell + gap)
            box = [x, y, x + cell, y + cell]
            if (row, col) == (0, 1):
                d.ellipse(box, fill=WHITE)
            else:
                d.rounded_rectangle(box, radius=r, fill=WHITE)
    return img


def main() -> None:
    full, small = tile(False), tile(True)
    sizes = [16, 20, 24, 32, 40, 48, 64, 128, 256]
    frames = [(small if s <= 24 else full).resize((s, s), Image.LANCZOS) for s in sizes]
    frames[-1].save(OUT / "app.ico", format="ICO", sizes=[(s, s) for s in sizes], append_images=frames[:-1])
    full.resize((256, 256), Image.LANCZOS).save(OUT / "icon-256.png")
    small.resize((32, 32), Image.LANCZOS).save(OUT / "icon-32.png")
    print("wrote", OUT / "app.ico")


if __name__ == "__main__":
    main()
