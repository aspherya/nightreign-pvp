"""Candidate single diagonal start routes over the playable island (tile rows Y 36..39), with terrain clearance over every map
variant layer (_00 base, _10.._50 Shifting Earth). Prints both diagonals; draws them on hawk_atlas2.png."""
import sys, os, math, json
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from nrmap import Msb, tiles
CELL = 16
base, top = {}, {}
for f, X, Y, a, b in tiles():
    if b != 0: continue
    for n, ty, p in Msb(f).parts():
        if p == (0.0, 0.0, 0.0) or p[1] < -100: continue          # skip pieces at origin and the crater pit layer's -668
        x = (X - 42) * 256 + 128 + p[0]; z = (Y - 34) * 256 + 128 + p[2]
        k = (int(x // CELL), int(z // CELL))
        top[k] = max(top.get(k, -1e9), p[1])
        if a == 0 and 36 <= Y <= 39: base.setdefault(k, []).append(p[1])
land = [k for k, v in base.items() if len(v) >= 3]
xs = sorted(k[0] * CELL for k in land); zs = sorted(k[1] * CELL for k in land)
x0, x1 = xs[len(xs) // 100], xs[-len(xs) // 100] + CELL
z0, z1 = zs[len(zs) // 100], zs[-len(zs) // 100] + CELL
print(f'playable land x {x0}..{x1}, z {z0}..{z1}')
M = 150
def profile(a, b):
    L = math.dist(a, b); hmax = -1e9; at = None
    for k in range(int(L // 8) + 1):
        x = a[0] + (b[0] - a[0]) * k * 8 / L; z = a[1] + (b[1] - a[1]) * k * 8 / L
        for du in (-1, 0, 1):
            for dv in (-1, 0, 1):
                h = top.get((int(x // CELL) + du, int(z // CELL) + dv))
                if h is not None and h > hmax: hmax, at = h, (round(x), round(z))
    return L, hmax, at
out = {}
for name, a, b in [('A (x0,z0) -> (x1,z1)', (x0 + M, z0 + M), (x1 - M, z1 - M)),
                   ('B (x1,z0) -> (x0,z1)', (x1 - M, z0 + M), (x0 + M, z1 - M))]:
    L, h, at = profile(a, b)
    print(f'{name}: {a} -> {b}, {L:.0f} m, {L / 17.4:.0f} s at 17.4 m/s; highest object under the line y {h:.0f} at {at}; fly at >= y {h + 60:.0f}')
    out[name[0]] = {'start': a, 'end': b, 'length': round(L), 'max_ground': round(h), 'fly_y': round(h + 60)}
json.dump(out, open('diagonals.json', 'w'), indent=1)
