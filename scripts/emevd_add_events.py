"""Append our event-script events to an ER/NR EMEVD (decompressed .emevd), in place. Used by scripts/build-gamedata.ps1 (UI-2).

Usage: python emevd_add_events.py <file.emevd> <events.json>

events.json: {"events": [{"id": 99100300, "flag": 10007100, "message": 99100200, "display": "dialog" | "blinking" | "status"}, ...]}
Each event: IF Event Flag(MAIN, ON, flag) [3:0] -> display(message) [2007:1 | 2007:4 | 2007:3] -> Set Event Flag(flag, OFF) [2003:66] ->
END Restart [1000:4]. Event 0 gets one Initialize Event(slot 0, id) [2000:0] per event.

Layout (SoulsFormats EMEVD, DS3/ER 64-bit; verified on NR 1.03.3 m10_00_00_00.emevd, see .claude/experiments/UI-0/emevd_dump.py):
header 0x90 | events (0x30 each) | instructions (0x20 each: bank i32, id i32, argsLen i64, argsOff i64 rel. to args, layerOff i64 = -1)
| args blob | parameters (0x20 each) | linked files (8 each) | strings. Event instruction offsets are relative to the instruction table,
argument offsets to the args blob, so appending to the end of each table keeps every existing offset valid. Event 0's instruction list is
copied to the end of the table with our Initialize Event lines added (the old copy stays, unreferenced); event 0 has no parameters.
Argument packing (DarkScript3 nr-common.emedf.json, checked against the file): 3:0 = s8 group, u8 state, u8 type, pad, u32 flag;
2003:66 = u8 type, pad3, u32 flag, u8 state, pad3; 1000:4 = u8 end type, pad3; 2000:0 = s32 slot, u32 event id, u32 param;
2007:1 = s32 msg, s16 type, s16 options, u32 entity, f32 distance; 2007:3 = s32 msg, u8 pad state, pad3; 2007:4 = s32 msg.
"""
import json, struct, sys

HEADER = 0x90
EV = 0x30
INS = 0x20


def build_event_instrs(ev):
    flag, msg, kind = ev['flag'], ev['message'], ev['display']
    out = [(3, 0, struct.pack('<bBBxI', 0, 1, 0, flag))]
    if kind == 'dialog':
        out.append((2007, 1, struct.pack('<ihhIf', msg, 0, 0, 0, 5.0)))
    elif kind == 'blinking':
        out.append((2007, 4, struct.pack('<i', msg)))
    elif kind == 'status':
        out.append((2007, 3, struct.pack('<iB3x', msg, 0)))
    else:
        raise SystemExit(f"unknown display {kind!r}")
    out.append((2003, 66, struct.pack('<B3xIB3x', 0, flag, 0)))
    out.append((1000, 4, struct.pack('<B3x', 1)))
    return out


def main(path, spec_path):
    b = bytearray(open(path, 'rb').read())
    if b[:4] != b'EVD\0' or b[4:8] != bytes.fromhex('00ff01ff'):
        raise SystemExit('not an ER/NR 64-bit EMEVD')
    q = lambda o: struct.unpack_from('<q', b, o)[0]
    h = {n: q(o) for n, o in [('eventCount', 0x10), ('eventsOff', 0x18), ('instrCount', 0x20), ('instrOff', 0x28),
                                ('unkCount', 0x30), ('unkOff', 0x38), ('layerCount', 0x40), ('layersOff', 0x48),
                                ('paramCount', 0x50), ('paramsOff', 0x58), ('linkedCount', 0x60), ('linkedOff', 0x68),
                                ('argsLen', 0x70), ('argsOff', 0x78), ('stringsLen', 0x80), ('stringsOff', 0x88)]}
    if h['unkCount'] or h['layerCount']:
        raise SystemExit('layers/unk tables not supported')
    if not (h['eventsOff'] == HEADER and h['instrOff'] == HEADER + EV * h['eventCount']
            and h['argsOff'] == h['instrOff'] + INS * h['instrCount'] and h['paramsOff'] == h['argsOff'] + h['argsLen']
            and h['linkedOff'] == h['paramsOff'] + 0x20 * h['paramCount'] and h['stringsOff'] == h['linkedOff'] + 8 * h['linkedCount']):
        raise SystemExit('unexpected section layout')
    spec = json.load(open(spec_path, encoding='utf-8'))['events']

    events = [bytearray(b[h['eventsOff'] + i * EV: h['eventsOff'] + (i + 1) * EV]) for i in range(h['eventCount'])]
    ids = {struct.unpack_from('<q', e, 0)[0] for e in events}
    instrs = [bytearray(b[h['instrOff'] + i * INS: h['instrOff'] + (i + 1) * INS]) for i in range(h['instrCount'])]
    args = bytearray(b[h['argsOff']: h['argsOff'] + h['argsLen']])
    rest = b[h['paramsOff']: h['stringsOff'] + h['stringsLen']]  # params + linked + strings, copied as is

    def add_instr(bank, iid, a):
        while len(args) % 4:
            args.append(0)
        off = len(args)
        args.extend(a)
        instrs.append(bytearray(struct.pack('<iiqqq', bank, iid, len(a), off, -1)))
        return len(instrs) - 1

    # Our events.
    for ev in spec:
        if ev['id'] in ids:
            raise SystemExit(f"event id {ev['id']} already exists")
        first = len(instrs)
        lines = build_event_instrs(ev)
        for bank, iid, a in lines:
            add_instr(bank, iid, a)
        events.append(bytearray(struct.pack('<qqqqqII', ev['id'], len(lines), first * INS, 0, -1, 0, 0)))

    # Event 0: copy its list to the end, add our Initialize Event lines.
    e0 = events[0]
    eid, ic, io, pc, po, rb, pad = struct.unpack('<qqqqqII', e0)
    if eid != 0 or pc != 0:
        raise SystemExit('event 0 missing or has parameters')
    first = len(instrs)
    for k in range(io // INS, io // INS + ic):
        instrs.append(bytearray(instrs[k]))
    for ev in spec:
        add_instr(2000, 0, struct.pack('<iII', 0, ev['id'], 0))
    events[0] = bytearray(struct.pack('<qqqqqII', 0, ic + len(spec), first * INS, 0, -1, rb, pad))

    while len(args) % 16:
        args.append(0)
    n_ev, n_in = len(events), len(instrs)
    events_off = HEADER
    instr_off = events_off + EV * n_ev
    args_off = instr_off + INS * n_in
    params_off = args_off + len(args)
    linked_off = params_off + 0x20 * h['paramCount']
    strings_off = linked_off + 8 * h['linkedCount']
    out = bytearray(b[:HEADER])
    for e in events:
        out += e
    for i in instrs:
        out += i
    out += args
    out += rest
    struct.pack_into('<I', out, 12, len(out))
    for name, val in [(0x10, n_ev), (0x18, events_off), (0x20, n_in), (0x28, instr_off), (0x38, args_off), (0x48, args_off),
                      (0x58, params_off), (0x68, linked_off), (0x70, len(args)), (0x78, args_off), (0x88, strings_off)]:
        struct.pack_into('<q', out, name, val)
    open(path, 'wb').write(out)
    print(f'{path}: +{len(spec)} events (now {n_ev}), +{n_in - h["instrCount"]} instructions, {len(b)} -> {len(out)} bytes')


if __name__ == '__main__':
    main(sys.argv[1], sys.argv[2])
