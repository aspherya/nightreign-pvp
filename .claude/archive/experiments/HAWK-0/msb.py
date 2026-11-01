"""Minimal MSB (ER/NR 'MSB ' v1) section walker: lists sections and entries with names."""
import struct, sys
def u16str(b, o):
    e = o
    while b[e:e+2] != b'\0\0': e += 2
    return b[o:e].decode('utf-16le')
def sections(b):
    o = 0x10
    out = []
    while o and o < len(b):
        ver, cnt = struct.unpack_from('<ii', b, o)
        name_off = struct.unpack_from('<q', b, o + 8)[0]
        offs = struct.unpack_from('<%dq' % cnt, b, o + 16)
        name = u16str(b, name_off)
        out.append((name, ver, list(offs[:-1])))
        o = offs[-1]
    return out
if __name__ == '__main__':
    b = open(sys.argv[1], 'rb').read()
    for name, ver, offs in sections(b):
        print(f'{name:24} ver={ver} entries={len(offs)}')
