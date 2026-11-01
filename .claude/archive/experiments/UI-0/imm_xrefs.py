"""Read-only: find instructions in nightreign.exe that use given 32-bit immediates (text ids), with their function (.pdata).

Usage: python imm_xrefs.py <exe> <id> [<id> ...]   (decimal ids)
For each hit prints the function range and the instructions around it.
"""
import bisect, re, struct, sys
import pefile
from capstone import Cs, CS_ARCH_X86, CS_MODE_64

exe = sys.argv[1]
ids = [int(a) for a in sys.argv[2:]]
pe = pefile.PE(exe, fast_load=True)
pe.parse_data_directories(directories=[pefile.DIRECTORY_ENTRY['IMAGE_DIRECTORY_ENTRY_EXCEPTION']])
img = pe.get_memory_mapped_image()
text = next(s for s in pe.sections if s.Name.rstrip(b'\0') == b'.text')
t0, t1 = text.VirtualAddress, text.VirtualAddress + text.Misc_VirtualSize
funcs = sorted((e.struct.BeginAddress, e.struct.EndAddress) for e in pe.DIRECTORY_ENTRY_EXCEPTION)
starts = [f[0] for f in funcs]
md = Cs(CS_ARCH_X86, CS_MODE_64)


def func_of(rva):
    i = bisect.bisect_right(starts, rva) - 1
    return funcs[i] if i >= 0 and funcs[i][0] <= rva < funcs[i][1] else None


for v in ids:
    pat = re.escape(struct.pack('<I', v))
    hits = [m.start() for m in re.finditer(pat, img[t0:t1])]
    print(f"##### {v} (0x{v:X}): {len(hits)} raw hits in .text")
    for h in hits[:12]:
        rva = t0 + h
        f = func_of(rva)
        # find an instruction that contains rva by decoding from a few starting points before it
        best = None
        for back in range(1, 12):
            s = rva - back
            ins = list(md.disasm(img[s:s + 40], s))
            for x in ins:
                if x.address <= rva < x.address + x.size and x.address + x.size >= rva + 4:
                    best = s
                    break
            if best is not None:
                break
        fdesc = f"func {f[0]:#x}..{f[1]:#x}" if f else "no .pdata"
        print(f"  hit {rva:#x} ({fdesc})")
        if best is None:
            continue
        start = max(best - 48, f[0] if f else best - 48)
        # decode forward from the function start if close, else from a guess, and print a window
        base = f[0] if f and rva - f[0] < 0x4000 else start
        window = [x for x in md.disasm(img[base:rva + 64], base)]
        idx = next((i for i, x in enumerate(window) if x.address <= rva < x.address + x.size), None)
        if idx is None:
            continue
        for x in window[max(0, idx - 6): idx + 6]:
            mark = "  <--" if x.address <= rva < x.address + x.size else ""
            print(f"     {x.address:#09x}  {x.mnemonic:6} {x.op_str}{mark}")
