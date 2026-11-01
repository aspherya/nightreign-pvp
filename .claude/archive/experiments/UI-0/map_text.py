"""UI-0 step 5: add the English text to each `ui_text_trace: TEXT` line of an nrpvp log, plus MARK lines, in time order.

Usage: python map_text.py <nrpvp.log> [msg work dir, default Z:/Tools/nr-unpacked/work/msg]
The msg dir holds WitchyBND unpacks (`*-msgbnd-dcx/_witchy-bnd4.xml` + `*.fmg.xml`) of game 1.03.3. Prints one line per text:
`<time> <FMG name> <category> <id> "<text>"`. Ids not found print `<not in FMG>` (e.g. Seamless Co-op's own strings).
"""
import glob, os, re, sys
import xml.etree.ElementTree as ET

log = sys.argv[1]
msgdir = sys.argv[2] if len(sys.argv) > 2 else "Z:/Tools/nr-unpacked/work/msg"
texts = {}
for manifest in glob.glob(os.path.join(msgdir, "*-msgbnd-dcx", "_witchy-bnd4.xml")):
    d = os.path.dirname(manifest)
    for f in ET.parse(manifest).getroot().iter("file"):
        cid, path = f.findtext("id"), f.findtext("path")
        if cid is None or path is None:
            continue
        x = os.path.join(d, path.replace("\\", "/").split("/")[-1] + ".xml")
        if not os.path.exists(x):
            continue
        for t in ET.parse(x).getroot().iter("text"):
            texts[(int(cid), int(t.get("id")))] = " ".join((t.text or "").split())

pat = re.compile(r"^(\S+).*ui_text_trace: TEXT (\S+) (\d+) id (\d+)")
mark = re.compile(r"^(\S+).*\bMARK\b(.*)$")
with open(log, encoding="utf-8", errors="replace") as fh:
    for line in fh:
        m = pat.search(line)
        if m:
            t, name, cat, tid = m.group(1), m.group(2), int(m.group(3)), int(m.group(4))
            s = texts.get((cat, tid))
            print(f"{t} {name} {cat} {tid} " + (f'"{s[:140]}"' if s is not None else "<not in FMG>"))
        elif "ui_text_trace:" in line and "TEXT" not in line:
            print(line.rstrip()[:220])
        else:
            m = mark.search(line)
            if m:
                print(f"{m.group(1)} ---- MARK{m.group(2)[:120]}")
