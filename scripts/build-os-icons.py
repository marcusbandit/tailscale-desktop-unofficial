#!/usr/bin/env python3
"""Rebuilds assets/os-icons.ttf, the OS and maker logos the badges draw.

Usage: scripts/build-os-icons.py SYMBOLS_TTF SIMPLE_ICONS_DIR

SYMBOLS_TTF is SymbolsNerdFont-Regular.ttf from a Nerd Fonts release
(NerdFontsSymbolsOnly). SIMPLE_ICONS_DIR holds motorola.svg, oneplus.svg,
xiaomi.svg, huawei.svg, nvidia.svg and amazon.svg from Simple Icons. Needs
fontTools. Keep UNICODES in step with src/osicon.rs.
"""
import re
import subprocess
import sys
import tempfile

from fontTools.pens.cu2quPen import Cu2QuPen
from fontTools.pens.transformPen import TransformPen
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.svgLib.path import parse_path
from fontTools.ttLib import TTFont

UNICODES = (
    "U+F300-F301,U+F303-F307,U+F309-F30A,U+F30C-F30E,U+F310,U+F312-F316,U+F318,"
    "U+F31A-F31B,U+F31D,U+F31F,U+F321-F322,U+F327-F32B,U+F32D-F32F,U+F331,U+F333,"
    "U+F337,U+F33F,U+F341-F346,U+F366,U+F374,U+F37D-F37E,U+F380,U+F382,U+F385,"
    "U+F17A,U+E8E5,U+F0A21,U+F179,U+F17B,U+F1A0,U+F10A-F10B,U+F0502,U+F0589,"
    "U+F1B6,U+F07D0,U+F08F3,U+F233"
)
BRANDS = [
    ("motorola", 0x100001),
    ("oneplus", 0x100002),
    ("xiaomi", 0x100003),
    ("huawei", 0x100004),
    ("nvidia", 0x100005),
    ("amazon", 0x100006),
]
OUT = "assets/os-icons.ttf"


def main(symbols, svg_dir):
    with tempfile.NamedTemporaryFile(suffix=".ttf") as base:
        subprocess.run(
            ["pyftsubset", symbols, f"--unicodes={UNICODES}", f"--output-file={base.name}",
             "--layout-features=", "--no-hinting", "--desubroutinize", "--name-IDs=*"],
            check=True,
        )
        # Read it all now; the temporary file goes when this block ends.
        font = TTFont(base.name, lazy=False)
        font.ensureDecompiled()
    # Brand logos take Tux's height and advance, so they sit like the rest.
    tux = font.getBestCmap()[0xF31A]
    ref = font["glyf"][tux]
    ref.recalcBounds(font["glyf"])
    top, height = ref.yMax, ref.yMax - ref.yMin
    adv = font["hmtx"][tux][0]
    for name, cp in BRANDS:
        d = re.search(r' d="([^"]+)"', open(f"{svg_dir}/{name}.svg").read()).group(1)
        scale = height / 24.0
        x0 = (adv - 24 * scale) / 2
        pen = TTGlyphPen(None)
        flip = TransformPen(Cu2QuPen(pen, 1.0, reverse_direction=True), (scale, 0, 0, -scale, x0, top))
        parse_path(d, flip)
        glyph = f"brand_{name}"
        # Setting a new glyph also adds it to the font's glyph order.
        font["glyf"][glyph] = pen.glyph()
        font["hmtx"][glyph] = (adv, int(x0))
        for table in font["cmap"].tables:
            if table.format == 12:
                table.cmap[cp] = glyph
    font["maxp"].numGlyphs = len(font.getGlyphOrder())
    font.save(OUT)
    print(f"wrote {OUT}")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    main(sys.argv[1], sys.argv[2])
