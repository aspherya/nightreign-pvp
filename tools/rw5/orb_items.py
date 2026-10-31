"""List MapItemMan item records of one running game: python items.py <instance>"""
import os, json, struct, subprocess, sys

NRO = r"C:\Users\Leonardo\source\repos\nightreign-pvp\.claude\worktrees\happy-goldstine-746484\target\release\nro.exe"
inst = sys.argv[1]


def cmd(line):
    out = subprocess.run([NRO, "--i", inst, "cmd", line], capture_output=True, text=True).stdout
    return json.loads(out)


def peek(addr, n):
    t = cmd(f"peek 0x{addr:X} 0x{n:X}")
    data = b""
    for l in t.split("\n")[1:]:
        for q in l.split(": ")[1].split():
            data += struct.pack("<Q", int(q, 16))
    return data


def q(d, off):
    return struct.unpack_from("<Q", d, off)[0]


slot = peek_slot = cmd("peek exe+0x3C10B68 8").split("+000: ")[1][:16]
mim = int(slot, 16)
head = peek(mim, 0x80)
print(f"MapItemMan 0x{mim:X}")
for off in range(0x48, 0x70, 8):
    p = q(head, off)
    if not p:
        continue
    d = peek(p, 0x100)
    iid = struct.unpack_from("<I", d, 0x28)[0]
    f78 = q(d, 0x78)
    h80 = q(d, 0x80)
    cnt = struct.unpack_from("<I", d, 0x20)[0]
    pos = None
    try:
        o1 = q(d, 0xE8)
        if o1:
            d1 = peek(o1, 0x100)
            o2 = q(d1, 0x88)
            if o2:
                d2 = peek(o2, 0xC0)
                pos = struct.unpack_from("<3f", d2, 0xA0)
                pos = (pos, 'node 0x%X' % (o2+0xA0))
    except Exception as e:
        pos = f"err {e}"
    print(f"+{off:02X} ptr 0x{p:X} id 0x{iid:08X} cnt {cnt} +78 {f78:016X} +80 {h80:016X} pos {pos}")
