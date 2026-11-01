"""S10: list printable ASCII (>=5) and UTF-16LE (>=5) strings of a PE with their section and RVA. Read-only."""
import sys, re, pefile
p = pefile.PE(sys.argv[1], fast_load=True)
data = open(sys.argv[1],'rb').read()
def sec(off):
    for s in p.sections:
        if s.PointerToRawData <= off < s.PointerToRawData + s.SizeOfRawData:
            return s.Name.rstrip(b'\0').decode(), s.VirtualAddress + off - s.PointerToRawData
    return '?', off
for m in re.finditer(rb'[\x20-\x7e]{5,}', data):
    n, rva = sec(m.start()); print(f"A {n} {rva:#x} {m.group().decode()}")
for m in re.finditer(rb'(?:[\x20-\x7e]\x00){5,}', data):
    n, rva = sec(m.start()); print(f"W {n} {rva:#x} {m.group().decode('utf-16le')}")
