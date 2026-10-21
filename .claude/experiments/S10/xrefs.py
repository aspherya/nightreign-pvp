"""S10: find RIP-relative references (lea/mov/cmp...) in a PE's executable sections to given target RVAs.
Usage: xrefs.py <pe> <rva_hex> [...]. Prints referencing instruction RVA and mnemonic only. Read-only."""
import sys, pefile, capstone
pe = pefile.PE(sys.argv[1], fast_load=True); targets = {int(a,16) for a in sys.argv[2:]}
md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64); md.skipdata = True
for s in pe.sections:
    if not s.Characteristics & 0x20000000: continue
    name = s.Name.rstrip(b'\0').decode(); data = s.get_data(); base = s.VirtualAddress
    # cheap scan: every 4-byte disp32 at offset i whose rip-relative target (i+4+disp+k) hits, k in 0..5 instruction tail
    import struct
    for i in range(0, len(data)-4):
        d = struct.unpack_from('<i', data, i)[0]
        for tail in (0,1,4):
            t = base + i + 4 + tail + d
            if t in targets:
                for back in range(2,8):
                    j = i-back
                    if j < 0: continue
                    for ins in md.disasm(data[j:j+16], base+j, 1):
                        if ins.size == back+4+tail and ins.op_str.find('rip') >= 0:
                            print(f"{name} {ins.address:#x} -> {t:#x}: {ins.mnemonic} {ins.op_str}")
