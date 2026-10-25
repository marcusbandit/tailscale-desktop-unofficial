#!/usr/bin/env python3
"""Rebuilds assets/os-icons.ttf, the OS and maker logos the badges draw.

Usage: scripts/build-os-icons.py SYMBOLS_TTF SIMPLE_ICONS_DIR

SYMBOLS_TTF is SymbolsNerdFont-Regular.ttf from a Nerd Fonts release
(NerdFontsSymbolsOnly). SIMPLE_ICONS_DIR holds the Simple Icons SVGs named in
BRANDS. Needs fontTools. The glyphs to keep are read from src/osicon.rs, so a
codepoint added there is picked up here.
"""
import re
import subprocess
import sys
import tempfile
from pathlib import Path

from fontTools.pens.cu2quPen import Cu2QuPen
from fontTools.pens.transformPen import TransformPen
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.svgLib.path import parse_path
from fontTools.ttLib import TTFont

ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "src" / "osicon.rs"
OUT = ROOT / "assets" / "os-icons.ttf"
# Maker logos live in Plane 16 private use; Nerd Fonts doesn't have them.
BRANDS = {
    0x100001: "motorola",
    0x100002: "oneplus",
    0x100003: "xiaomi",
    0x100004: "huawei",
    0x100005: "nvidia",
    0x100006: "amazon",
}


def codepoints():
    found = {int(c, 16) for c in re.findall(r"\\u\{([0-9a-fA-F]+)\}", SOURCE.read_text())}
    missing = {c for c in found if c >= 0x100000} - BRANDS.keys()
    if missing:
        sys.exit(f"no SVG named for {', '.join(f'U+{c:X}' for c in sorted(missing))}")
    return sorted(c for c in found if c < 0x100000)


def main(symbols, svg_dir):
    unicodes = ",".join(f"U+{c:X}" for c in codepoints())
    with tempfile.NamedTemporaryFile(suffix=".ttf") as base:
        subprocess.run(
            [sys.executable, "-m", "fontTools.subset", symbols, f"--unicodes={unicodes}",
             f"--output-file={base.name}", "--layout-features=", "--no-hinting",
             "--desubroutinize", "--name-IDs=*"],
            check=True,
        )
        # Read it all now; the temporary file goes when this block ends.
        font = TTFont(base.name, lazy=False)
        font.ensureDecompiled()
    # Maker logos fit the box Tux fills, centred, so they sit like the rest.
    tux = font.getBestCmap()[0xF31A]
    glyf = font["glyf"]
    ref = glyf[tux]
    ref.recalcBounds(glyf)
    adv = font["hmtx"][tux][0]
    height = ref.yMax - ref.yMin
    scale = min(height, adv) / 24.0
    x0 = (adv - 24 * scale) / 2
    top = ref.yMax - (height - 24 * scale) / 2
    for cp, name in BRANDS.items():
        svg = (Path(svg_dir) / f"{name}.svg").read_text()
        d = re.search(r' d="([^"]+)"', svg).group(1)
        pen = TTGlyphPen(None)
        flip = TransformPen(Cu2QuPen(pen, 1.0, reverse_direction=True), (scale, 0, 0, -scale, x0, top))
        parse_path(d, flip)
        glyph = f"brand_{name}"
        # Setting a new glyph also adds it to the font's glyph order.
        glyf[glyph] = pen.glyph()
        glyf[glyph].recalcBounds(glyf)
        font["hmtx"][glyph] = (adv, glyf[glyph].xMin)
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
