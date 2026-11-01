"""Read-only: find code that loads the MsgRepository global (exe+0x3C246A8) and what it calls next."""
import re, struct, sys, collections
import pefile
from capstone import Cs, CS_ARCH_X86, CS_MODE_64

exe, target = sys.argv[1], int(sys.argv[2], 16)
pe = pefile.PE(exe, fast_load=True)
img = pe.get_memory_mapped_image()
md = Cs(CS_ARCH_X86, CS_MODE_64)
pat = re.compile(rb'[\x48\x4C]\x8B[\x05\x0D\x15\x1D\x25\x2D\x35\x3D]', re.S)
sites = []
for m in pat.finditer(img):
    i = m.start()
    disp = struct.unpack_from('<i', img, i + 3)[0]
    if i + 7 + disp == target:
        sites.append(i)
print(f"{len(sites)} loads of {target:#x}")
callees = collections.Counter()
examples = collections.defaultdict(list)
for s in sites:
    seq = []
    for x in md.disasm(img[s:s + 80], s):
        seq.append(x)
        if x.mnemonic in ('call', 'jmp', 'ret'):
            break
    last = seq[-1]
    if last.mnemonic in ('call', 'jmp') and last.op_str.startswith('0x'):
        c = int(last.op_str, 16)
        callees[c] += 1
        imms = [f"{x.mnemonic} {x.op_str}" for x in seq[1:-1] if re.search(r'(edx|r8d|r9d), (0x[0-9a-f]+|\d+)$', x.op_str)]
        if len(examples[c]) < 6:
            examples[c].append((s, imms))
for c, n in callees.most_common(8):
    print(f"callee {c:#x}: {n} sites")
    for s, imms in examples[c]:
        print(f"   site {s:#x}: {imms}")
