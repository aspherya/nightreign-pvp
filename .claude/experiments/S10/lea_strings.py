"""S10: in a PE code range, list rip-relative lea targets that point at printable strings. Usage: lea_strings.py <pe> <start_rva> <end_rva>"""
import sys, re, pefile, capstone
pe = pefile.PE(sys.argv[1]); a, b = int(sys.argv[2],16), int(sys.argv[3],16)
img = pe.get_memory_mapped_image()
md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64); md.detail = True
def s_at(rva):
    m = re.match(rb'[\x20-\x7e]{4,200}', img[rva:rva+200]); 
    if m: return m.group().decode()
    m = re.match(rb'(?:[\x20-\x7e]\x00){4,100}', img[rva:rva+200])
    return ('W:'+m.group().decode('utf-16le')) if m else None
for ins in md.disasm(img[a:b], a):
    if ins.mnemonic in ('lea','mov','movups','movdqu') and 'rip' in ins.op_str:
        for op in ins.operands:
            if op.type == capstone.x86.X86_OP_MEM and op.mem.base == capstone.x86.X86_REG_RIP:
                t = ins.address + ins.size + op.mem.disp; s = s_at(t)
                if s: print(f"{ins.address:#x} {t:#x} {s[:120]}")
