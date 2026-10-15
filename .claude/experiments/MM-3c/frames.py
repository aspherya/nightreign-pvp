"""Read-only: for each return-address RVA, find its function in nightreign.exe (.pdata) and disassemble around the call site."""
import bisect, struct, sys
import pefile
from capstone import Cs, CS_ARCH_X86, CS_MODE_64

exe = sys.argv[1]
rvas = [int(a, 16) for a in sys.argv[2:]]
pe = pefile.PE(exe, fast_load=True)
pe.parse_data_directories(directories=[pefile.DIRECTORY_ENTRY['IMAGE_DIRECTORY_ENTRY_EXCEPTION']])
img = pe.get_memory_mapped_image()
base = pe.OPTIONAL_HEADER.ImageBase
funcs = sorted((e.struct.BeginAddress, e.struct.EndAddress) for e in pe.DIRECTORY_ENTRY_EXCEPTION)
starts = [f[0] for f in funcs]
md = Cs(CS_ARCH_X86, CS_MODE_64)
md.detail = False


def func_of(rva):
    i = bisect.bisect_right(starts, rva) - 1
    if i >= 0 and funcs[i][0] <= rva < funcs[i][1]:
        return funcs[i]
    return None


def dis(start, end):
    return list(md.disasm(img[start:end], start))


for rva in rvas:
    f = func_of(rva)
    print(f"=== ret {rva:#x}: function {f[0]:#x}..{f[1]:#x} (size {f[1]-f[0]:#x})" if f else f"=== ret {rva:#x}: no .pdata entry")
    if not f:
        continue
    ins = dis(f[0], f[1])
    idx = next((i for i, x in enumerate(ins) if x.address == rva), None)
    if idx is None:
        print("   (return address not on an instruction boundary from the function start)")
        continue
    for x in ins[max(0, idx - 14): idx + 2]:
        mark = "  <-- call" if x.address + x.size == rva else ""
        print(f"   {x.address:#09x}  {x.mnemonic:6} {x.op_str}{mark}")
    calls = [x for x in ins if x.mnemonic == "call"]
    print(f"   function has {len(ins)} instructions, {len(calls)} calls")
