"""Find RTTI vtables by class name and code that references them (constructors)."""
import re, struct, sys, nr
IMGBASE = 0x140000000
def td_of(cls):
    out = []
    for m in re.finditer(re.escape(b".?AV" + cls.encode() + b"@"), nr.img):
        out.append(m.start() - 16)
    return out
def cols_of(td):
    # COL: sig(4)=1, off, cdOff, pTypeDescriptor(rva), pClassHierarchy(rva), pSelf(rva)
    pat = struct.pack("<I", td)
    out = []
    for m in re.finditer(re.escape(pat), nr.img):
        c = m.start() - 12
        if nr.u32(c) == 1 and nr.u32(c + 20) == c: out.append(c)
    return out
def vtables_of(cls):
    vts = []
    for td in td_of(cls):
        for c in cols_of(td):
            pat = struct.pack("<Q", IMGBASE + c)
            for m in re.finditer(re.escape(pat), nr.img):
                vts.append((m.start() + 8, nr.u32(c + 4)))
    return vts
if __name__ == "__main__":
    for cls in sys.argv[1:]:
        for vt, off in vtables_of(cls):
            refs = nr.xrefs(vt)
            print(f"{cls}: vtable {vt:#x} (offset {off:#x}) refs:", [hex(r) for r, k in refs][:12])
            for r, k in refs[:12]:
                f = nr.func_of(r); print(f"   ref {r:#x} in func {f[0]:#x}..{f[1]:#x}" if f else f"   ref {r:#x} (no pdata)")
