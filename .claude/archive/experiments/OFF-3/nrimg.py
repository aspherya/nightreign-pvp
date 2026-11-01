"""Pure-python helpers (no numpy): mapped image cache, string search, RIP-relative xrefs, call xrefs, disasm."""
import os, re, struct, bisect, pefile
from capstone import Cs, CS_ARCH_X86, CS_MODE_64
EXE = r"Z:\SteamLibrary\steamapps\common\ELDEN RING NIGHTREIGN\Game\nightreign.exe"
import tempfile
HERE = tempfile.gettempdir()  # cache outside the repo (~130 MB)
RAW = os.path.join(HERE, 'nr-1.3.3.0.img'); META = os.path.join(HERE, 'nr-1.3.3.0.meta')
class Img:
    def __init__(self):
        if not os.path.exists(RAW):
            pe = pefile.PE(EXE, fast_load=True)
            pe.parse_data_directories(directories=[pefile.DIRECTORY_ENTRY['IMAGE_DIRECTORY_ENTRY_EXCEPTION']])
            open(RAW, 'wb').write(pe.get_memory_mapped_image())
            funcs = sorted((e.struct.BeginAddress, e.struct.EndAddress) for e in getattr(pe, 'DIRECTORY_ENTRY_EXCEPTION', []))
            secs = [(s.Name.rstrip(b'\0').decode(errors='replace'), s.VirtualAddress, s.Misc_VirtualSize) for s in pe.sections]
            open(META, 'w').write(repr({'secs': secs, 'funcs': funcs}))
        m = eval(open(META).read()); self.secs = m['secs']; self.funcs = m['funcs']
        self.starts = [f[0] for f in self.funcs]
        self.img = open(RAW, 'rb').read()
        self.md = Cs(CS_ARCH_X86, CS_MODE_64)
    def sec(self, n): return next(s for s in self.secs if s[0] == n)
    def text_ranges(self):
        return [(va, va+sz) for (n, va, sz) in self.secs if n.startswith('.text') or n in ('.arxan', '.srdata') ]
    def func_of(self, rva):
        i = bisect.bisect_right(self.starts, rva) - 1
        return self.funcs[i] if i >= 0 and self.funcs[i][0] <= rva < self.funcs[i][1] else None
    def find(self, data): return [m.start() for m in re.finditer(re.escape(data), self.img)]
    def cstr(self, rva, n=200): return self.img[rva:rva+n].split(b'\0')[0].decode(errors='replace')
    def ripxrefs(self, target, ranges=None):
        """Any instruction with a disp32 at its end-ish (tail 0/1/4 bytes of immediate) that resolves to target."""
        out = []
        ranges = ranges or [(va, va+sz) for (n, va, sz) in self.secs if n == '.text']
        # opcode-driven: ModRM with mod=00 rm=101 (rip) -> modrm & 0xC7 == 0x05
        pat = re.compile(rb'[\x05\x0D\x15\x1D\x25\x2D\x35\x3D]', re.S)
        for lo, hi in ranges:
            buf = self.img
            for m in pat.finditer(buf, lo, hi - 4):
                j = m.start()
                d = struct.unpack_from('<i', buf, j+1)[0]
                for tail in (0, 1, 4):
                    if j + 5 + tail + d == target:
                        for back in (1, 2, 3):
                            st = j - back
                            for x in self.md.disasm(buf[st:st+16], st, 1):
                                if x.size == back + 5 + tail and 'rip' in x.op_str:
                                    out.append((x.address, f'{x.mnemonic} {x.op_str}'))
        return sorted(set(out))
    def callers(self, target, ranges=None):
        ranges = ranges or [(va, va+sz) for (n, va, sz) in self.secs if n == '.text']
        out = []
        for lo, hi in ranges:
            for m in re.finditer(rb'[\xE8\xE9]', self.img[lo:hi-8]):
                j = lo + m.start()
                if j + 5 + struct.unpack_from('<i', self.img, j+1)[0] == target:
                    out.append((j, 'call' if self.img[j] == 0xE8 else 'jmp'))
        return out
    def dis(self, start, n=60):
        for x in self.md.disasm(self.img[start:start+n*15], start):
            n -= 1
            if n < 0: break
            print(f'  {x.address:#09x}  {x.mnemonic:7} {x.op_str}')
