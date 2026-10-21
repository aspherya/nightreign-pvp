"""S10: for every .pdata function of a PE, list rip-relative references to printable strings, as 'func_rva insn_rva target string'.
Usage: fn_strrefs.py <pe>. Read-only; output names/strings only."""
import sys, re, pefile, capstone
pe = pefile.PE(sys.argv[1]); img = pe.get_memory_mapped_image()
md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64); md.detail = True
def s_at(rva):
    m = re.match(rb'[\x20-\x7e]{4,160}\x00', img[rva:rva+161])
    if m: return m.group()[:-1].decode()
    m = re.match(rb'(?:[\x20-\x7e]\x00){4,100}\x00\x00', img[rva:rva+204])
    return ('W:'+m.group()[:-2].decode('utf-16le')) if m else None
seen=set()
for e in pe.DIRECTORY_ENTRY_EXCEPTION:
    b, en = e.struct.BeginAddress, e.struct.EndAddress
    if b in seen: continue
    seen.add(b)
    for ins in md.disasm(img[b:en], b):
        if 'rip' not in ins.op_str: continue
        for op in ins.operands:
            if op.type == capstone.x86.X86_OP_MEM and op.mem.base == capstone.x86.X86_REG_RIP:
                t = ins.address + ins.size + op.mem.disp
                if 0 < t < len(img):
                    s = s_at(t)
                    if s: print(f"{b:#x} {ins.address:#x} {t:#x} {s[:140]}")
