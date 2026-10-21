"""Dump bird routes (EVENT type 25) of an NR MSB: name, entity id, type-data, point chain with positions and lengths."""
import struct, sys, math
from msb import sections, u16str
b = open(sys.argv[1], 'rb').read()
S = dict((n, offs) for n, v, offs in sections(b))
def name(o): return u16str(b, o + struct.unpack_from('<q', b, o)[0])
pts = S['POINT_PARAM_ST']
def point(k):
    o = pts[k]
    return name(o), struct.unpack_from('<3f', b, o + 0x14), struct.unpack_from('<f', b, o + 0x24)[0]
allp = []
for o in S['EVENT_PARAM_ST']:
    if struct.unpack_from('<i', b, o + 0xC)[0] != 25: continue
    common = o + struct.unpack_from('<q', b, o + 0x18)[0]
    td = o + struct.unpack_from('<q', b, o + 0x20)[0]
    eid = struct.unpack_from('<I', b, common + 8)[0]
    head = struct.unpack_from('<4i', b, td)
    idx = []
    for k in range(64):
        v = struct.unpack_from('<h', b, td + 0x10 + 2 * k)[0]
        if v < 0: break
        idx.append(v)
    chain = [point(k) for k in idx]
    L = sum(math.dist((p[1][0], p[1][2]), (q[1][0], q[1][2])) for p, q in zip(chain, chain[1:]))
    print(f'{name(o)} | entity {eid} | td {head} | {len(idx)} pts | {L:.0f} m')
    for n, (x, y, z), h in chain:
        print(f'     {x:9.1f} {y:7.1f} {z:9.1f} h{h:7.1f}  {n}')
