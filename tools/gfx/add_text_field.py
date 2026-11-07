"""Add a static text field to the root timeline of a menu .gfx exported by JPEXS (re/GFX_UI.md).

Usage (Python 3, no packages):
    python tools/gfx/add_text_field.py IN.xml OUT.xml --id 713 --depth 900 --name NrpvpKills \
        --text "KILLS 3" --x 1660 --y 100 [--size 28] [--color FFC850]

IN.xml comes from `ffdec-cli.exe -swf2xml file.gfx IN.xml`; OUT.xml goes back with `ffdec-cli.exe -xml2swf OUT.xml file.gfx`.
--id must be a character id the file does not use (one above the largest), --depth a root depth no other object uses; x / y are
pixels on the 1920x1080 stage, size is the font size in pixels, the font is the game's MenuFont_01 (imported from font.swf).
The field is inserted before the root timeline's last ShowFrame (the menus have one root frame). Verified with 01_000_fe.gfx
(the HUD) on game 1.03.3, 2026-11-07: the text shows on the HUD.
"""

import argparse
import re


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("inp")
    ap.add_argument("out")
    ap.add_argument("--id", type=int, required=True)
    ap.add_argument("--depth", type=int, required=True)
    ap.add_argument("--name", required=True)
    ap.add_argument("--text", required=True)
    ap.add_argument("--x", type=int, required=True)
    ap.add_argument("--y", type=int, required=True)
    ap.add_argument("--size", type=int, default=28)
    ap.add_argument("--color", default="FFC850")
    a = ap.parse_args()

    s = open(a.inp, encoding="utf-8").read()
    used = {int(m) for m in re.findall(r'(?:characterI[dD]|spriteId|shapeId)="(\d+)"', s)}
    if a.id in used:
        raise SystemExit(f"character id {a.id} is already used (largest is {max(used)})")
    if re.search(rf'^    <item type="PlaceObject\dTag"[^>]* depth="{a.depth}"', s, re.M):
        raise SystemExit(f"root depth {a.depth} is already used")
    marker = '\n    <item type="ShowFrameTag" forceWriteAsLong="false"/>\n'
    at = s.rfind(marker)
    if at < 0:
        raise SystemExit("no root ShowFrame found")
    r, g, b = (int(a.color[i:i + 2], 16) for i in (0, 2, 4))
    twips = 20
    text = a.text.replace("&", "&amp;").replace('"', "&quot;").replace("<", "&lt;")
    new = f'''
    <item type="DefineEditTextTag" align="0" autoSize="false" border="false" characterID="{a.id}" fontClass="MenuFont_01" fontHeight="{a.size * twips}" forceWriteAsLong="true" hasFont="false" hasFontClass="true" hasLayout="true" hasMaxLength="false" hasText="true" hasTextColor="true" html="false" indent="0" initialText="{text}" leading="0" leftMargin="0" multiline="false" noSelect="true" password="false" readOnly="true" rightMargin="0" useOutlines="true" variableName="" wasStatic="false" wordWrap="false">
      <bounds type="RECT" Xmax="{a.size * twips * 12}" Xmin="-40" Ymax="{a.size * twips + 240}" Ymin="-40" nbits="16"/>
      <textColor type="RGBA" alpha="255" blue="{b}" green="{g}" red="{r}"/>
    </item>
    <item type="PlaceObject2Tag" characterId="{a.id}" depth="{a.depth}" forceWriteAsLong="true" name="{a.name}" placeFlagHasCharacter="true" placeFlagHasClipActions="false" placeFlagHasClipDepth="false" placeFlagHasColorTransform="false" placeFlagHasMatrix="true" placeFlagHasName="true" placeFlagHasRatio="false" placeFlagMove="false">
      <matrix type="MATRIX" hasRotate="false" hasScale="false" nRotateBits="0" nScaleBits="0" nTranslateBits="17" translateX="{a.x * twips}" translateY="{a.y * twips}"/>
    </item>'''
    open(a.out, "w", encoding="utf-8").write(s[:at] + new + s[at:])
    print(f"added {a.name!r} (character {a.id}, depth {a.depth}) at ({a.x}, {a.y})")


if __name__ == "__main__":
    main()
