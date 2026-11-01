"""Read-only: disassemble functions (by .pdata) at the given RVAs; max N instructions each."""
import bisect, sys
import pefile
from capstone import Cs, CS_ARCH_X86, CS_MODE_64
exe = sys.argv[1]; n = int(sys.argv[2]); rvas = [int(a, 16) for a in sys.argv[3:]]
pe = pefile.PE(exe, fast_load=True)
pe.parse_data_directories(directories=[pefile.DIRECTORY_ENTRY['IMAGE_DIRECTORY_ENTRY_EXCEPTION']])
img = pe.get_memory_mapped_image()
funcs = sorted((e.struct.BeginAddress, e.struct.EndAddress) for e in pe.DIRECTORY_ENTRY_EXCEPTION)
starts = [f[0] for f in funcs]
md = Cs(CS_ARCH_X86, CS_MODE_64)
for rva in rvas:
    i = bisect.bisect_right(starts, rva) - 1
    f = funcs[i] if i >= 0 and funcs[i][0] <= rva < funcs[i][1] else (rva, rva + 0x200)
    print(f"=== {rva:#x} in function {f[0]:#x}..{f[1]:#x}")
    for k, x in enumerate(md.disasm(img[f[0]:f[1]], f[0])):
        if k >= n: break
        print(f"   {x.address:#09x}  {x.bytes.hex():<22} {x.mnemonic:6} {x.op_str}")
