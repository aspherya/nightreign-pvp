import nr, sys, re
def strat(r):
    b = nr.img[r:r+80]
    if b[:1].isascii() and re.match(rb'[ -~]{4,}\0', b): return b.split(b'\0')[0].decode()
    if re.match(rb'(?:[ -~]\0){4,}\0\0', b): return b.decode('utf-16le','replace').split('\0')[0]
    return None
def summ(s, e, maxn=400):
    out = []
    for x in nr.dis(s, e)[:maxn]:
        line = None
        if x.mnemonic == 'call' or x.mnemonic.startswith('j') and 'ptr' not in x.op_str and x.op_str.startswith('0x') and not (s <= int(x.op_str,16) < e):
            line = f"{x.address:#x} {x.mnemonic} {x.op_str}"
        m = re.search(r'\[rip \+ (0x[0-9a-f]+)\]', x.op_str)
        if m:
            t = x.address + x.size + int(m.group(1), 16)
            st = strat(t)
            line = f"{x.address:#x} {x.mnemonic} {x.op_str} -> {t:#x}" + (f' "{st}"' if st else '')
        if re.search(r'0x9a[4-6][0-9a-f]\]|0x9a40|0x1e5e0|0x1e658', x.op_str): line = (line or f"{x.address:#x} {x.mnemonic} {x.op_str}") + "   <WCM>"
        if line: out.append(line)
    return out
if __name__ == "__main__":
    for a in sys.argv[1:]:
        f = nr.func_of(int(a,16)); print(f"== {a} func {f[0]:#x}..{f[1]:#x}")
        print("\n".join(summ(*f)))
