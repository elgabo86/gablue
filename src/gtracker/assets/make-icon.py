#!/usr/bin/python3
"""Dessine l'icône de GTracker et ses exports.

- `assets/icon.rgba` : 128×128 RGBA brute, embarquée dans la fenêtre par egui
  (`egui::IconData`, barre des tâches) ;
- `assets/icon-512.png` : référence lisible ;
- `assets/icon.svg` + copie installée dans l'image
  (`files/system/all/usr/share/icons/hicolor/scalable/apps/gtracker.svg`).

Un seul dessin, trois sorties : relancer après toute retouche
(`assets/make-icon.py`). Le tracé est supersampleté (4×) puis réduit, donc les
bords sont lissés au lieu d'être des pixels collés à la main (v1, 20/11).

Design : tuile sombre arrondie, onde carrée épaisse (la signature chiptune) et
pastille « lecture » cyan. Deux éléments seulement — à 16 px un troisième
détail ne se lit plus.
"""

import pathlib

from PIL import Image, ImageDraw, ImageFilter

S = 1024          # maître
SS = 4            # supersampling
W = S * SS

BG_TOP = (0x24, 0x2B, 0x3B, 255)
BG_BOTTOM = (0x12, 0x15, 0x1C, 255)
EDGE = (0x3A, 0x43, 0x56, 255)
MINT = (0x46, 0xE6, 0xA0, 255)
CYAN = (0x3F, 0xD8, 0xFF, 255)
INK = (0x10, 0x13, 0x19, 255)

RADIUS = int(W * 0.235)
WAVE_W = int(W * 0.085)     # épaisseur du trait de l'onde


def rounded_tile():
    """Tuile à dégradé vertical, coins arrondis, liseré intérieur."""
    top = Image.new("RGBA", (W, W), BG_TOP)
    bottom = Image.new("RGBA", (W, W), BG_BOTTOM)
    mask = Image.new("L", (W, W), 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, W - 1, W - 1], RADIUS, fill=255)

    tile = Image.new("RGBA", (W, W), (0, 0, 0, 0))
    grad = Image.new("RGBA", (W, W))
    px = grad.load()
    for y in range(W):
        t = y / (W - 1)
        c = tuple(round(BG_TOP[i] + (BG_BOTTOM[i] - BG_TOP[i]) * t) for i in range(3))
        for x in range(0, W, 8):         # bandes de 8 px, invisible à la réduction
            for dx in range(8):
                if x + dx < W:
                    px[x + dx, y] = (*c, 255)
    tile.paste(grad, (0, 0), mask)

    # liseré : contour arrondi plus clair, à 1,5 % du bord
    inset = int(W * 0.012)
    ring = Image.new("L", (W, W), 0)
    ImageDraw.Draw(ring).rounded_rectangle(
        [inset, inset, W - 1 - inset, W - 1 - inset], RADIUS - inset, outline=255,
        width=int(W * 0.008),
    )
    edge = Image.new("RGBA", (W, W), EDGE)
    tile.paste(edge, (0, 0), ring)

    out = Image.new("RGBA", (W, W), (0, 0, 0, 0))
    out.paste(tile, (0, 0), mask)
    return out


def wave_points():
    """Onde carrée propre : crêtes et creux horizontaux, arêtes verticales.

    Deux périodes, amplitude 1/4 de la tuile. Une onde *vraie* (créneaux
    horizontaux) se lit mieux qu'une ligne qui descend en escalier — le premier
    jet (20/11) ressemblait à un électrocardiogramme.
    """
    x0, x1 = W * 0.150, W * 0.700
    hi, lo = W * 0.430, W * 0.610
    period = (x1 - x0) / 2.0
    pts = [(x0, lo)]
    for k in range(2):
        b = x0 + k * period
        pts += [
            (b + period * 0.5, lo),
            (b + period * 0.5, hi),
            (b + period, hi),
            (b + period, lo),
        ]
    return pts


def draw_wave(tile):
    """Onde mint + halo doux (lueur), trait épais à bouts arrondis."""
    layer = Image.new("RGBA", (W, W), (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    pts = wave_points()
    d.line(pts, fill=MINT, width=WAVE_W, joint="curve")
    r = WAVE_W / 2
    for x, y in (pts[0], pts[-1]):
        d.ellipse([x - r, y - r, x + r, y + r], fill=MINT)

    glow = layer.filter(ImageFilter.GaussianBlur(W * 0.018))
    glow.putalpha(glow.getchannel("A").point(lambda a: int(a * 0.40)))
    tile.alpha_composite(glow)
    tile.alpha_composite(layer)


def draw_badge(tile):
    """Pastille « lecture » : disque sombre, anneau mint, triangle cyan."""
    cx, cy = W * 0.800, W * 0.780
    r = W * 0.118
    layer = Image.new("RGBA", (W, W), (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    d.ellipse([cx - r, cy - r, cx + r, cy + r], fill=INK)
    d.ellipse(
        [cx - r, cy - r, cx + r, cy + r],
        outline=MINT, width=int(W * 0.028),
    )
    # triangle : décalé d'un poil à droite, comme un vrai bouton play
    t = r * 0.60
    d.polygon(
        [
            (cx - t * 0.55, cy - t * 0.85),
            (cx - t * 0.55, cy + t * 0.85),
            (cx + t * 0.85, cy),
        ],
        fill=CYAN,
    )
    tile.alpha_composite(layer)


def render():
    tile = rounded_tile()
    draw_wave(tile)
    draw_badge(tile)
    return tile.resize((S, S), Image.LANCZOS)


def ascii_preview(img, size=32):
    small = img.resize((size, size), Image.LANCZOS).convert("RGBA")
    out = []
    for y in range(size):
        row = ""
        for x in range(size):
            r, g, b, a = small.getpixel((x, y))
            if a < 40:
                row += " "
            elif b > 170 and g > 140:
                row += "@"          # cyan
            elif g > 120 and r < 140:
                row += "#"          # mint
            elif r + g + b < 110:
                row += "."
            else:
                row += ":"
        out.append(row)
    return "\n".join(out)


def svg():
    """Même dessin, en vectoriel : `Icon=` du .desktop pointe dessus."""
    pts = wave_points()
    k = S / W
    d = "M " + " L ".join(f"{x * k:.1f} {y * k:.1f}" for x, y in pts)
    cx, cy, r = 0.800 * S, 0.780 * S, 0.118 * S
    t = r * 0.62
    return f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {S} {S}" width="{S}" height="{S}">
  <defs>
    <linearGradient id="bg" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#242b3b"/>
      <stop offset="1" stop-color="#12151c"/>
    </linearGradient>
  </defs>
  <rect x="0" y="0" width="{S}" height="{S}" rx="{0.235 * S:.0f}" ry="{0.235 * S:.0f}" fill="url(#bg)"/>
  <rect x="{0.012 * S:.0f}" y="{0.012 * S:.0f}" width="{0.976 * S:.0f}" height="{0.976 * S:.0f}"
        rx="{0.223 * S:.0f}" ry="{0.223 * S:.0f}" fill="none" stroke="#3a4356" stroke-width="{0.008 * S:.0f}"/>
  <path d="{d}" fill="none" stroke="#46e6a0" stroke-width="{0.085 * S:.0f}"
        stroke-linecap="round" stroke-linejoin="round"/>
  <circle cx="{cx:.0f}" cy="{cy:.0f}" r="{r:.0f}" fill="#101319" stroke="#46e6a0" stroke-width="{0.028 * S:.0f}"/>
  <path d="M {cx - t * 0.55:.0f} {cy - t * 0.85:.0f} L {cx - t * 0.55:.0f} {cy + t * 0.85:.0f} L {cx + t * 0.85:.0f} {cy:.0f} Z"
        fill="#3fd8ff"/>
</svg>
"""


def main():
    here = pathlib.Path(__file__).resolve().parent
    img = render()

    img.resize((512, 512), Image.LANCZOS).save(here / "icon-512.png")
    # RGBA brute, pas un PNG : `include_bytes!` + `egui::IconData` attendent
    # 128×128×4 octets tels quels (un `.save()` PIL écrirait un en-tête PNG,
    # l'icône de fenêtre deviendrait du bruit — vécu 20/11).
    (here / "icon.rgba").write_bytes(img.resize((128, 128), Image.LANCZOS).tobytes())

    text = svg()
    (here / "icon.svg").write_text(text)
    installed = here.parents[2] / "files/system/all/usr/share/icons/hicolor/scalable/apps/gtracker.svg"
    if installed.parent.exists():
        installed.write_text(text)
        print(f"SVG installé : {installed.relative_to(here.parents[2])}")

    print(f"icône écrite dans {here}")
    print(ascii_preview(img))


if __name__ == "__main__":
    main()
