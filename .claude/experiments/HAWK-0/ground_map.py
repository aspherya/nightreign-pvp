"""HAWK route atlas: ground samples from every part in the Limveld base tiles (m60_4X_3Y_00), drawn with every hawk route of
m60_00_00_99 into hawk_atlas.png. Frame (fit_frame2.py, 2026-10-22): x99 = (X-42)*256 + 128 + lx, z99 = (Y-34)*256 + 128 + lz,
y unchanged. Writes ground_cells.json: {"cell": 16, "cells": {"i,j": [ground_y, n_samples]}} (ground_y = 20th percentile)."""
import re, sys, os, math, json, struct, zlib
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from nrmap import Msb, tiles
CELL = 16
samples = {}
for f, X, Y, a, b in tiles():
    if (a, b) != (0, 0): continue
    for n, ty, p in Msb(f).parts():
        if p == (0.0, 0.0, 0.0): continue
        x = (X - 42) * 256 + 128 + p[0]; z = (Y - 34) * 256 + 128 + p[2]
        samples.setdefault((int(x // CELL), int(z // CELL)), []).append(p[1])
cells = {}
for k, ys in samples.items():
    ys.sort(); cells[k] = (ys[len(ys) // 5], len(ys))
json.dump({'cell': CELL, 'cells': {f'{i},{j}': v for (i, j), v in cells.items()}}, open('ground_cells.json', 'w'))
routes = list(Msb('m60_00_00_99.msb.dcx').routes())

# --- raster: 2 m per pixel over x -200..1500, z 150..1850 ---
X0, Z0, W, H, S = -200, 150, 850, 850, 2.0
img = [[(18, 32, 56)] * W for _ in range(H)]          # sea / no data
ys = [v[0] for v in cells.values()]
lo, hi = sorted(ys)[len(ys) // 50], sorted(ys)[-len(ys) // 50]
def px(x, z): return int((x - X0) / S), int(H - 1 - (z - Z0) / S)   # z up
for (i, j), (gy, n) in cells.items():
    t = max(0.0, min(1.0, (gy - lo) / (hi - lo)))
    col = (int(60 + 150 * t), int(110 + 110 * t), int(60 + 120 * t))
    for xx in range(int(CELL / S)):
        for zz in range(int(CELL / S)):
            u, v = px(i * CELL + xx * S, j * CELL + zz * S)
            if 0 <= u < W and 0 <= v < H: img[v][u] = col
def dot(x, z, col, r=3):
    u0, v0 = px(x, z)
    for du in range(-r, r + 1):
        for dv in range(-r, r + 1):
            if du * du + dv * dv <= r * r and 0 <= u0 + du < W and 0 <= v0 + dv < H: img[v0 + dv][u0 + du] = col
def line(a, b, col):
    n = int(max(abs(a[0] - b[0]), abs(a[1] - b[1])) / S) + 1
    for k in range(n + 1):
        dot(a[0] + (b[0] - a[0]) * k / n, a[1] + (b[1] - a[1]) * k / n, col, 1)
for n, c in routes:
    start = '出撃' in n
    if '不使用' in n or 'NT専用' in n: col = (120, 120, 120)
    elif start: col = (255, 220, 40)
    else: col = (80, 200, 255)
    for p, q in zip(c, c[1:]): line((p[0], p[2]), (q[0], q[2]), col)
    if start and col != (120, 120, 120): dot(c[-1][0], c[-1][2], (255, 120, 0), 4)
for x, z in [(1052, 1134), (650, 549)]: dot(x, z, (255, 0, 0), 6)     # HAWK-1b falls to y -26
for gx in range(0, 1501, 256):                                        # tile grid ticks
    for v in range(H):
        u, _ = px(gx - 128, Z0)
        if 0 <= u < W and v % 6 == 0: img[v][u] = (90, 90, 90)
if os.path.exists('diagonals.json'):                                    # candidate single routes (diagonal.py)
    for key, col in (('A', (255, 0, 255)), ('B', (255, 255, 255))):
        d = json.load(open('diagonals.json'))[key]
        line(tuple(d['start']), tuple(d['end']), col)
        dot(*d['start'], col, 5)
for x in (16, 1008):                                                   # playable land box (rows Y 36..39)
    line((x, 528), (x, 1520), (200, 200, 200))
for z in (528, 1520):
    line((16, z), (1008, z), (200, 200, 200))
raw = b''.join(b'\0' + bytes(c for p in row for c in p) for row in img)
def chunk(t, d): return struct.pack('>I', len(d)) + t + d + struct.pack('>I', zlib.crc32(t + d) & 0xffffffff)
open('hawk_atlas.png', 'wb').write(b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', W, H, 8, 2, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(raw, 9)) + chunk(b'IEND', b''))
print('cells', len(cells), 'height p2..p98', round(lo), round(hi))
for x, z in [(1052, 1134), (650, 549)]:
    k = (int(x // CELL), int(z // CELL))
    near = [cells[(k[0] + a, k[1] + b)] for a in range(-2, 3) for b in range(-2, 3) if (k[0] + a, k[1] + b) in cells]
    print('fall point', (x, z), 'ground cells within 32 m:', len(near), 'heights', sorted(round(v[0]) for v in near)[:6])
