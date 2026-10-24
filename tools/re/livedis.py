"""Disassemble live game memory through the mod console (read-only `peek`). Usage: livedis.py <addr|exe+off> <len> [mark-addr ...]"""
import json, struct, subprocess, sys, capstone, os
NRO = os.path.join(os.path.dirname(__file__), '..', '..', 'target', 'debug', 'nro.exe')
def peek(spec, n):
    out = b''
    off = 0
    while off < n:
        k = min(1024, n - off)
        s = spec if off == 0 else f"{spec}+{hex(off)}"
        r = json.loads(subprocess.run([NRO, 'cmd', 'peek', s, hex(k)], capture_output=True, text=True).stdout)
        lines = r.split('\n')
        if off == 0:
            base = int(lines[0].split()[0], 16)
        for l in lines[1:]:
            for x in l.split(': ')[1].split():
                out += struct.pack('<Q', int(x, 16))
        off += k
    return base, out[:n]
if __name__ == '__main__':
    base, b = peek(sys.argv[1], int(sys.argv[2], 0))
    marks = {int(m, 16) for m in sys.argv[3:]}
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
    for x in md.disasm(b, base):
        m = '  <==' if (x.address + x.size) in marks or x.address in marks else ''
        print(f'  {x.address:#x} {x.mnemonic} {x.op_str}{m}')
