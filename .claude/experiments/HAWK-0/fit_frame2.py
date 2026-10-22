import re, sys, os, math, json
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from nrmap import Msb, tiles
land = {}
for n, c in Msb('m60_00_00_99.msb.dcx').routes():
    m = re.search(r'出撃ルート(\d\d)', n)
    if m and '不使用' not in n and 'NT専用' not in n:
        a, b = c[-2], c[-1]
        L = math.dist((a[0], a[2]), (b[0], b[2]))
        ux, uz = (b[0] - a[0]) / L, (b[2] - a[2]) / L
        land.setdefault(m.group(1), []).append((b[0] + 52 * ux, b[2] + 52 * uz))
ret = {}
for f, X, Y, a, b in tiles():
    if (a, b) != (0, 0): continue
    for n, p in Msb(f).points():
        m = re.search(r'出撃ルート(\d\d) 復帰用ポイント①', n)
        if m: ret[m.group(1)] = (X, Y, p)
res = []
for dx in range(-600, 601, 4):
    for dz in range(-600, 601, 4):
        cost = []
        for k, (X, Y, p) in ret.items():
            x = (X - 42) * 256 + p[0] + dx; z = (Y - 34) * 256 + p[2] + dz
            cost.append(min(math.dist((x, z), e) for e in land[k]))
        res.append((sorted(cost)[len(cost) // 2], sum(c < 25 for c in cost), dx, dz, [round(c) for c in cost]))
res.sort()
for r in res[:6]: print(r)
