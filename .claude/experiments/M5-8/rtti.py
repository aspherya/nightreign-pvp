import nr
def name(vt):
    col = nr.u64(vt - 8) - 0x140000000
    if not (0 < col < len(nr.img)): return None
    td = nr.u32(col + 12)
    s = nr.img[td + 16: td + 16 + 200]
    return s.split(b'\0')[0].decode('ascii', 'replace')
