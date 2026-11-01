"""Read-only helpers for nightreign.exe 1.3.3.0 (M5-4, for M5-8): cached image, .pdata, disasm, byte search, rip-relative xrefs.
Needs capstone, pefile, numpy (pip --user). Cache (img.bin, ~118 MB) goes to %TEMP%\nr-m58, outside the repo. Usage:
  python -c "import nr; nr.around(0x674FCD)"   (run from this folder)
"""
import bisect, os, re, struct, sys, pickle
from capstone import Cs, CS_ARCH_X86, CS_MODE_64
EXE = r"Z:\SteamLibrary\steamapps\common\ELDEN RING NIGHTREIGN\Game\nightreign.exe"
HERE = os.path.join(os.environ.get("TEMP", "."), "nr-m58")
os.makedirs(HERE, exist_ok=True)
IMG = os.path.join(HERE, "img.bin"); META = os.path.join(HERE, "meta.pkl")
if not os.path.exists(IMG):
    import pefile
    pe = pefile.PE(EXE, fast_load=True)
    pe.parse_data_directories(directories=[pefile.DIRECTORY_ENTRY['IMAGE_DIRECTORY_ENTRY_EXCEPTION']])
    open(IMG, "wb").write(pe.get_memory_mapped_image())
    funcs = sorted((e.struct.BeginAddress, e.struct.EndAddress) for e in pe.DIRECTORY_ENTRY_EXCEPTION)
    secs = [(s.Name.rstrip(b"\0").decode(), s.VirtualAddress, s.Misc_VirtualSize) for s in pe.sections]
    pickle.dump((funcs, secs), open(META, "wb"))
img = open(IMG, "rb").read()
funcs, secs = pickle.load(open(META, "rb"))
starts = [f[0] for f in funcs]
md = Cs(CS_ARCH_X86, CS_MODE_64)
def func_of(rva):
    i = bisect.bisect_right(starts, rva) - 1
    if i >= 0 and funcs[i][0] <= rva < funcs[i][1]: return funcs[i]
def sec(name):
    for n, va, sz in secs:
        if n == name: return va, sz
def dis(start, end):
    return list(md.disasm(img[start:end], start))
def show(start, end, mark=()):
    for x in dis(start, end):
        m = "  <==" if x.address in mark else ""
        print(f"  {x.address:#09x}  {x.mnemonic:7} {x.op_str}{m}")
def around(rva, before=0x40, after=0x40):
    f = func_of(rva)
    s = f[0] if f else rva - before
    ins = dis(s, (f[1] if f else rva + after))
    idx = next((i for i, x in enumerate(ins) if x.address == rva), None)
    if idx is None:
        show(rva - before, rva + after, (rva,)); return
    lo = max(0, idx - before // 4); hi = idx + after // 4
    for x in ins[lo:hi]:
        m = "  <==" if x.address == rva else ""
        print(f"  {x.address:#09x}  {x.mnemonic:7} {x.op_str}{m}")
def find(pat, sect=".text"):
    va, sz = sec(sect)
    rx = re.compile(pat, re.S)
    return [va + m.start() for m in rx.finditer(img, va, va + sz)]
def u32(r): return struct.unpack_from("<I", img, r)[0]
def i32(r): return struct.unpack_from("<i", img, r)[0]
def u64(r): return struct.unpack_from("<Q", img, r)[0]
def calls_to(target):
    """rel32 call sites to target RVA in .text"""
    va, sz = sec(".text"); out = []
    for m in re.finditer(rb"\xE8", img[va:va+sz]):
        r = va + m.start()
        if r + 5 + i32(r + 1) == target: out.append(r)
    return out
_np = None
def xrefs(target, extra=(0,1,2,4)):
    """rip-relative references: disp32 at i with i+4+k+disp == target, k = bytes after disp (imm)."""
    global _np
    import numpy as np
    va, sz = sec(".text")
    if _np is None:
        _np = np.frombuffer(img, dtype=np.uint8)
    a = _np[va:va+sz+3]
    d = (a[:-3].astype(np.int64) | (a[1:-2].astype(np.int64) << 8) | (a[2:-1].astype(np.int64) << 16) | (a[3:].astype(np.int64) << 24))
    d = np.where(d >= 2**31, d - 2**32, d)
    idx = np.arange(va, va + sz, dtype=np.int64)
    out = []
    for k in extra:
        hits = np.nonzero(idx + 4 + k + d == target)[0]
        out += [(int(va + h), k) for h in hits]
    return sorted(out)
