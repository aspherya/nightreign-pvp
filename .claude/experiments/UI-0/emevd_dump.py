"""Read-only EMEVD (ER/NR 64-bit) dumper: header, event 0, and sample instructions of given (bank, id).

Usage: python emevd_dump.py <file.emevd> [bank:id ...]
Header offsets (verified on NR 1.03.3 m10: instruction table at 0x20/0x28, args at 0x78) follow SoulsFormats EMEVD (DS3 layout).
"""
import struct, sys

f = sys.argv[1]
want = {tuple(int(x) for x in a.split(':')) for a in sys.argv[2:]}
b = open(f, 'rb').read()
q = lambda o: struct.unpack_from('<q', b, o)[0]
hdr = {n: q(o) for n, o in [('eventCount', 0x10), ('eventsOff', 0x18), ('instrCount', 0x20), ('instrOff', 0x28),
                             ('unkCount', 0x30), ('unkOff', 0x38), ('layerCount', 0x40), ('layersOff', 0x48),
                             ('paramCount', 0x50), ('paramsOff', 0x58), ('linkedCount', 0x60), ('linkedOff', 0x68),
                             ('argsLen', 0x70), ('argsOff', 0x78), ('stringsLen', 0x80), ('stringsOff', 0x88)]}
print('magic', b[:4], 'flags', b[4:8].hex(), 'version', struct.unpack_from('<I', b, 8)[0], 'fileSize', struct.unpack_from('<I', b, 12)[0], 'len', len(b))
for k, v in hdr.items():
    print(f'  {k} = {v:#x}')


def instr(k):
    bank, iid, alen, aoff, loff = struct.unpack_from('<iiqqq', b, hdr['instrOff'] + k * 32)
    args = b[hdr['argsOff'] + aoff: hdr['argsOff'] + aoff + alen] if alen > 0 else b''
    return bank, iid, alen, aoff, loff, args


EV = 0x30
for e in range(min(hdr['eventCount'], 3)):
    eid, ic, io, pc, po, rest, pad = struct.unpack_from('<qqqqqII', b, hdr['eventsOff'] + e * EV)
    print(f'event[{e}] id={eid} instrs={ic} instrOff={io:#x} params={pc} paramsOff={po:#x} rest={rest}')
    if e == 0:
        first = io // 32
        for k in range(first, first + min(ic, 12)):
            bank, iid, alen, aoff, loff, args = instr(k)
            print(f'   {bank}[{iid}] len={alen} argOff={aoff:#x} layer={loff} args={args.hex()}')
shown = {}
for k in range(hdr['instrCount']):
    bank, iid, alen, aoff, loff, args = instr(k)
    if (bank, iid) in want and shown.get((bank, iid), 0) < 3:
        shown[(bank, iid)] = shown.get((bank, iid), 0) + 1
        print(f'sample {bank}[{iid}] #{k} len={alen} layer={loff} args={args.hex()}')
# parameter entries (size check)
if hdr['paramCount']:
    print('paramsOff..linkedOff span', hdr['linkedOff'] - hdr['paramsOff'], 'per param', (hdr['linkedOff'] - hdr['paramsOff']) / hdr['paramCount'])
print('section order:', sorted([(v, k) for k, v in hdr.items() if k.endswith('Off')]))
