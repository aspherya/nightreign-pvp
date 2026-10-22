"""HAWK-1b: write m60_00_00_99.msb.dcx with every start-hawk route stretched across Limveld.

Usage: patch_routes.py <oo2core_9_win64.dll> <vanilla m60_00_00_99.msb.dcx> <out .msb.dcx> [--margin 50] [--dry]

Each start route (EVENT type 25 named 出撃ルート, not marked 【不使用】 = unused) keeps its start point. Its other points move onto the
line start -> vanilla end (horizontal direction kept), spread evenly up to `margin` metres inside the land outline, all at the start
height, so the hawk flies one long straight leg and the engine's own route end drops the player at the far side.
Land outline: the convex hull (x, z) of every vanilla start-route end point (a vanilla drop spot) and every Spectral Hawk Tree route
point, all over land. HAWK-1b run 1 (2026-10-22) ended route 08_000 at (1052, 1134), inside the old box x -334..1202, z 256..1792
(TECHNICAL_NOTES 4c), and dropped the player into the void; the land points span only x -24..1032, z 467..1541.
In-place float edits, so the file size does not change.
"""
import ctypes, math, os, struct, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from msb import sections, u16str


def dcx_unpack(oodle, b):
    assert b[:4] == b'DCX\0' and b[0x28:0x2C] == b'KRAK', 'not a DCX_KRAK file'
    usize, csize = struct.unpack('>II', b[0x1C:0x24])
    i = b.index(b'DCA\0')
    data = b[i + 8:i + 8 + csize]
    out = ctypes.create_string_buffer(usize)
    n = oodle.OodleLZ_Decompress(data, len(data), out, usize, 0, 0, 0, None, 0, None, None, None, 0, 3)
    assert n == usize, f'decompress {n} != {usize}'
    return b[:i + 8], out.raw


def dcx_pack(oodle, header, raw):
    level = header[0x30]  # DCP KRAK level byte (6 in the vanilla file)
    cap = len(raw) + 0x10000
    comp = ctypes.create_string_buffer(cap)
    n = oodle.OodleLZ_Compress(8, raw, len(raw), comp, level, None, None, None, None, 0)  # 8 = Kraken
    assert n > 0, 'compress failed'
    h = bytearray(header)
    h[0x1C:0x24] = struct.pack('>II', len(raw), n)
    body = bytes(h) + comp.raw[:n]
    return body + b'\0' * (-len(body) % 16)


def convex_hull(ps):
    ps = sorted(set(ps))
    cross = lambda o, a, b: (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    lo, hi = [], []
    for p in ps:
        while len(lo) >= 2 and cross(lo[-2], lo[-1], p) <= 0: lo.pop()
        lo.append(p)
    for p in reversed(ps):
        while len(hi) >= 2 and cross(hi[-2], hi[-1], p) <= 0: hi.pop()
        hi.append(p)
    return lo[:-1] + hi[:-1]  # counter-clockwise


def hull_distance(p, d, hull, m):
    """Distance from p along unit 2D direction d to where it leaves the convex hull shrunk by m metres."""
    t = math.inf
    for (ax, az), (bx, bz) in zip(hull, hull[1:] + hull[:1]):
        ex, ez = bx - ax, bz - az
        n = (ez / math.hypot(ex, ez), -ex / math.hypot(ex, ez))  # outward normal of a counter-clockwise edge
        nd = n[0] * d[0] + n[1] * d[1]
        if nd > 1e-9:
            t = min(t, (n[0] * ax + n[1] * az - m - n[0] * p[0] - n[1] * p[1]) / nd)
    return t


def main():
    sys.stdout.reconfigure(encoding='utf-8')
    args = [a for a in sys.argv[1:] if not a.startswith('--')]
    margin = float(sys.argv[sys.argv.index('--margin') + 1]) if '--margin' in sys.argv else 50.0
    if '--margin' in sys.argv: args.remove(sys.argv[sys.argv.index('--margin') + 1])
    dry = '--dry' in sys.argv
    oodle_path, src, dst = args
    oodle = ctypes.WinDLL(oodle_path)
    oodle.OodleLZ_Decompress.restype = ctypes.c_int64
    oodle.OodleLZ_Decompress.argtypes = [ctypes.c_void_p, ctypes.c_int64, ctypes.c_void_p, ctypes.c_int64] + [ctypes.c_int64] * 3 + [ctypes.c_void_p, ctypes.c_int64, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_int64, ctypes.c_int64]
    oodle.OodleLZ_Compress.restype = ctypes.c_int64
    oodle.OodleLZ_Compress.argtypes = [ctypes.c_int, ctypes.c_void_p, ctypes.c_int64, ctypes.c_void_p, ctypes.c_int, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_int64]

    header, raw = dcx_unpack(oodle, open(src, 'rb').read())
    b = bytearray(raw)
    S = dict((n, offs) for n, v, offs in sections(raw))
    pts = S['POINT_PARAM_ST']
    name = lambda o: u16str(raw, o + struct.unpack_from('<q', raw, o)[0])

    routes = []  # (name, entity, point indices)
    use = {}
    land = []
    for o in S['EVENT_PARAM_ST']:
        if struct.unpack_from('<i', raw, o + 0xC)[0] != 25: continue
        common = o + struct.unpack_from('<q', raw, o + 0x18)[0]
        td = o + struct.unpack_from('<q', raw, o + 0x20)[0]
        idx = []
        for k in range(64):
            v = struct.unpack_from('<h', raw, td + 0x10 + 2 * k)[0]
            if v < 0: break
            idx.append(v)
        for v in idx: use[v] = use.get(v, 0) + 1
        n = name(o)
        P = [struct.unpack_from('<3f', raw, pts[k] + 0x14) for k in idx]
        land += [(P[-1][0], P[-1][2])] if '出撃ルート' in n else [(q[0], q[2]) for q in P]
        if '出撃ルート' in n and '不使用' not in n:
            routes.append((n, struct.unpack_from('<I', raw, common + 8)[0], idx))

    hull = convex_hull(land)
    print(f'{len(routes)} start routes, margin {margin:.0f} m, land outline {len(hull)} corners from {len(land)} points')
    for n, eid, idx in routes:
        shared = [v for v in idx[1:] if use[v] > 1]
        assert not shared, f'{n}: points {shared} are used by other routes too'
        p = [struct.unpack_from('<3f', raw, pts[k] + 0x14) for k in idx]
        sx, sy, sz = p[0]
        dx, dz = p[-1][0] - sx, p[-1][2] - sz
        L0 = math.hypot(dx, dz)
        d = (dx / L0, dz / L0)
        L = hull_distance((sx, sz), d, hull, margin)
        assert L > L0, f'{n}: new length {L:.0f} m is not longer than vanilla {L0:.0f} m'
        for j, k in enumerate(idx[1:], 1):
            t = L * j / (len(idx) - 1)
            struct.pack_into('<3f', b, pts[k] + 0x14, sx + d[0] * t, sy, sz + d[1] * t)
        ex, ez = sx + d[0] * L, sz + d[1] * L
        print(f'{n:28} entity {eid}  {L0:5.0f} m -> {L:5.0f} m ({L / 17:4.0f} s at 17 m/s)  '
              f'start ({sx:7.1f},{sy:6.1f},{sz:7.1f})  end ({ex:7.1f},{sy:6.1f},{ez:7.1f})')

    assert len(b) == len(raw)
    if dry: return
    out = dcx_pack(oodle, header, bytes(b))
    _, check = dcx_unpack(oodle, out)
    assert check == bytes(b), 'round-trip mismatch'
    os.makedirs(os.path.dirname(os.path.abspath(dst)), exist_ok=True)
    open(dst, 'wb').write(out)
    print(f'ok: {dst} ({len(out)} bytes, round-trip checked)')


if __name__ == '__main__':
    main()
