"""S10: disassemble a PE from an RVA (reads the file section directly). Usage: disasm_at.py <pe> <rva_hex> <len> [skip-bad]. Read-only; for our notes only."""
import sys, pefile, capstone
pe = pefile.PE(sys.argv[1], fast_load=True)
a, n = int(sys.argv[2],16), int(sys.argv[3],0)
off = pe.get_offset_from_rva(a)
with open(sys.argv[1],'rb') as f: f.seek(off); buf = f.read(n)
md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64); md.skipdata = True
for i in md.disasm(buf, a):
    print(f"{i.address:#x}: {i.bytes.hex():24} {i.mnemonic} {i.op_str}")
