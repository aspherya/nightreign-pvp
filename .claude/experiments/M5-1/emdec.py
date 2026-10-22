"""Read-only EMEVD decompiler-lite for NR 1.03.3: prints each event with instruction names and decoded args (DarkScript3 EMEDF).
Usage: python emdec.py <file.emevd> [event_id ...]   (no ids = all events)
"""
import json, struct, sys

ED = json.load(open(r'Z:\Tools\DarkScript3-3.6.3\Resources\nr-common.emedf.json', encoding='utf-8'))
NAMES = {}
for c in ED['main_classes']:
    for i in c['instrs']:
        NAMES[(c['index'], i['index'])] = (i['name'], i['args'])
ENUMS = {e['name']: e['values'] for e in ED['enums']}
TY = {0: ('B', 1), 1: ('H', 2), 2: ('I', 4), 3: ('b', 1), 4: ('h', 2), 5: ('i', 4), 6: ('f', 4), 8: ('I', 4)}


def load(path):
    b = open(path, 'rb').read()
    q = lambda o: struct.unpack_from('<q', b, o)[0]
    h = {n: q(o) for n, o in [('eventCount', 0x10), ('eventsOff', 0x18), ('instrCount', 0x20), ('instrOff', 0x28),
                                ('paramCount', 0x50), ('paramsOff', 0x58), ('argsLen', 0x70), ('argsOff', 0x78)]}
    return b, h


def decode_args(bank, iid, a):
    nm, spec = NAMES.get((bank, iid), (None, None))
    if spec is None:
        return f'{bank}[{iid}]', a.hex()
    out, off = [], 0
    for s in spec:
        fmt, sz = TY.get(s['type'], ('I', 4))
        while off % sz:
            off += 1
        if off + sz > len(a):
            out.append(f"{s['name']}=?")
            continue
        v = struct.unpack_from('<' + fmt, a, off)[0]
        off += sz
        en = s.get('enum_name')
        if en and en in ENUMS and str(v) in ENUMS[en]:
            v = f"{ENUMS[en][str(v)]}({v})"
        elif isinstance(v, float):
            v = round(v, 3)
        out.append(f"{s['name']}={v}")
    return f'{nm} {bank}[{iid}]', ', '.join(out)


def events(path):
    b, h = load(path)
    res = {}
    for e in range(h['eventCount']):
        eid, ic, io, pc, po, rest = struct.unpack_from('<qqqqqI', b, h['eventsOff'] + e * 0x30)
        params = {}
        for p in range(pc):
            ii, tgt, src, cnt, _u = struct.unpack_from('<qqqii', b, h['paramsOff'] + po + p * 0x20)
            params.setdefault(ii, []).append((tgt, src, cnt))
        lines = []
        for k in range(ic):
            bank, iid, alen, aoff, loff = struct.unpack_from('<iiqqq', b, h['instrOff'] + io + k * 32)
            a = b[h['argsOff'] + aoff: h['argsOff'] + aoff + alen] if alen > 0 else b''
            nm, ar = decode_args(bank, iid, a)
            ps = ''
            if k in params:
                ps = '  <- ' + ', '.join(f'[{t}]=X{s}_{c}' for t, s, c in params[k])
            lines.append(f'  {k:3d} {nm}: {ar}{ps}')
        res[eid] = (rest, lines)
    return res


def _init_args(a):
    n = len(a) // 4
    return ', '.join(str(x) for x in struct.unpack_from('<' + 'i' * n, a, 0))


_orig = decode_args


def decode_args(bank, iid, a):  # noqa: F811  show every parameter of Initialize (Common) Event
    if bank == 2000 and iid in (0, 6):
        return ('Initialize Event' if iid == 0 else 'Initialize Common Event') + f' {bank}[{iid}]', _init_args(a)
    return _orig(bank, iid, a)


if __name__ == '__main__':
    want = {int(x) for x in sys.argv[2:]}
    for eid, (rest, lines) in events(sys.argv[1]).items():
        if want and eid not in want:
            continue
        print(f'EVENT {eid} rest={rest}')
        print('\n'.join(lines))
