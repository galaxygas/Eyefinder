#!/usr/bin/env python3
"""Synthetic map fixtures for crates/ef-map/tests (no game data), and their reference.

    python3 tools/fixtures/map_fixtures.py crates/ef-map/tests/fixtures

Written from the documented layouts only:

* GTA1 `.CMP` and `.GRY`: DMA Design, CityScape Data Structure 12.10, sections 2.1-2.9 and 3.6-3.7
  (https://misc.daniel-marschall.de/spiele/gta1/cds.pdf).
* GTA2 `.gmp`: DMA Design, GBH Map Format 8.2 (https://gtamp.com/gta2/gta2-gmp-map-file-format/);
  `.sty`: GBH Style 4.6 (as tools/txd-ref/style_fixtures.py); `.seq`: the labels gta2_re
  (CriminalRETeam/gta2_re @ edb484d3) Frontend.cpp GetSeqItem_4B48D0 reads.
* Radar dictionaries: RenderWare D3D8 TXDs as tools/txd-ref/make_fixtures.py writes them, in IMG
  archives (v1 for GTA III, VER2 for San Andreas).

The reference (map_reference.json) gives the expected top-down and radar images as FNV-1a hashes of
their RGBA pixels. It is computed here in image order from the pixel patterns, with the lid
orientation tables written out below from their sources (OpenGTA for GTA1, gta2_re for GTA2),
independently of the Rust parser and renderer.
"""
import json
import os
import struct
import sys


# RenderWare and IMG writers, as in tools/txd-ref/make_fixtures.py (which runs on import).
def chunk(t, payload, ver):
    return struct.pack("<III", t, len(payload), ver) + payload


def name32(s):
    return s.encode().ljust(32, b"\0")


def txd(natives, device, ver):
    body = chunk(1, struct.pack("<HH", len(natives), device), ver) + b"".join(natives) + chunk(3, b"", ver)
    return chunk(0x16, body, ver)


def img_v1(entries):
    """GTA III / VC: names + data -> (.dir bytes, .img bytes). 2048-byte sectors."""
    d, img = b"", b""
    for name, data in entries:
        sectors = (len(data) + 2047) // 2048
        d += struct.pack("<II", len(img) // 2048, sectors) + name.encode().ljust(24, b"\0")
        img += data.ljust(sectors * 2048, b"\0")
    return d, img


def img_v2(entries):
    """San Andreas VER2: header + 32-byte entries, data from sector after the directory."""
    header_len = 8 + 32 * len(entries)
    first = (header_len + 2047) // 2048
    d, body, sector = b"", b"", first
    for name, data in entries:
        sectors = (len(data) + 2047) // 2048
        d += struct.pack("<IHH", sector, sectors, 0) + name.encode().ljust(24, b"\0")
        body += data.ljust(sectors * 2048, b"\0")
        sector += sectors
    head = (b"VER2" + struct.pack("<I", len(entries)) + d).ljust(first * 2048, b"\0")
    return head + body


def fnv(data):
    h = 0xcbf29ce484222325
    for b in data:
        h = ((h ^ b) * 0x100000001b3) & 0xffffffffffffffff
    return f"{h:016x}"


def write(path, data):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    open(path, 'wb').write(data)


# --- GTA1 -------------------------------------------------------------------------------------------

G1_LIDS = 3  # lid graphics 0..2; lid value 0 means "no face", so the map uses 1 and 2


def g1_index(tile, x, y):
    return (tile * 41 + x * 3 + y * 7) % 256


def g1_style():
    count = 1 + G1_LIDS  # one side tile, three lid tiles, no aux tiles
    tiles = bytearray(((count + 3) // 4) * 16384)
    for t in range(count):
        for y in range(64):
            for x in range(64):
                tiles[(t // 4) * 16384 + y * 256 + (t % 4) * 64 + x] = g1_index(t, x, y)
    palette = bytes(v for c in range(256) for v in (c, (c * 3) % 256, 255 - c))
    remaps = bytes((c + r) % 256 for r in range(256) for c in range(256))
    index = bytes([0, 1, 2, 3]) * 256
    sizes = [325, 4096, G1_LIDS * 4096, 0, 1, len(palette), len(remaps), len(index), 0, 0, 0, 0, 42]
    body = bytes(tiles) + b'\0' + palette + remaps + index + bytes(42)
    return struct.pack('<13I', *sizes) + body


def g1_rgba(lid, remap):
    """Lid graphic `lid` with remap value `remap` (CityScape 2.1 x3-4, 3.3, 3.4), image order."""
    out = []
    for y in range(64):
        for x in range(64):
            i = g1_index(1 + lid, x, y)
            if remap:
                table = [0, 1, 2, 3][remap]  # remap_index[lid][remap]
                i = (i + table) % 256
            out.append((i, (i * 3) % 256, 255 - i, 255 if i else 0))
    return out


# (x, y, [blocks from ground up as (lid, rotation, remap, flat)])
G1_COLUMNS = [
    (10, 20, [(1, 0, 0, False)]),
    (11, 20, [(2, 1, 0, False)]),
    (12, 20, [(1, 2, 1, False)]),
    (13, 20, [(2, 3, 0, False)]),
    (10, 21, [(1, 0, 0, False), (2, 0, 0, True)]),
    (11, 21, [(0, 0, 0, False)]),  # a block with no lid
]


def g1_cmp():
    blocks = [bytes(8)]  # block 0: air, no faces
    columns = bytearray(struct.pack('<H', 6))  # offset 0: the empty column (height 6 = no blocks)
    base = [0] * 65536
    for x, y, stack in G1_COLUMNS:
        nums = []
        for lid, rot, remap, flat in stack:
            type_map = (2 << 4) | (0x80 if flat else 0) | (rot << 14)
            ext = remap << 3
            blocks.append(struct.pack('<HB5B', type_map, ext, 0, 0, 0, 0, lid))
            nums.append(len(blocks) - 1)
        base[y * 256 + x] = len(columns)
        # Top block first: OpenGTA (read_cmp.cpp getBlockAtNew) reads blockd[N-1] as the lowest block,
        # although CityScape 2.4 says blockd[0] is the ground-level one (docs/FORMATS.md).
        columns += struct.pack('<H', 6 - len(nums)) + struct.pack(f'<{len(nums)}H', *reversed(nums))
    objects = struct.pack('<3HBB3H', 640, 1280, 256, 3, 0, 100, 0, 0)
    objects += struct.pack('<3HBB3H', 700, 1300, 256, 1, 130, 200, 0, 0)  # remap >= 128: a car
    route = struct.pack('<BB', 2, 255) + bytes([10, 20, 4, 11, 20, 4])
    location = bytearray(108)
    location[0:3] = bytes([10, 20, 4])  # police station 1
    nav = struct.pack('<5B', 10, 20, 4, 2, 7) + b'Downtown'.ljust(30, b'\0')
    nav += struct.pack('<5B', 0, 0, 255, 255, 1) + b'Liberty'.ljust(30, b'\0')
    head = struct.pack('<IBBH5I', 331, 1, 1, 0, len(route), len(objects), len(columns), len(blocks) * 8, len(nav))
    return (head + struct.pack('<65536I', *base) + bytes(columns) + b''.join(blocks) + objects + route
            + bytes(location) + nav)


# OpenGTA @ 1ae34ae3 gl_cityview.cpp drawBlock / slope1_data.h: the flat lid's corners are
# (x0,y1) (x1,y1) (x1,y0) (x0,y0) = SW, SE, NE, NW and take the texture corners
# (0,1) (1,1) (1,0) (0,0), starting `rotation` corners later. Texel (u, v) at each map corner:
G1_CORNERS = {  # rotation -> texel corner at NW, NE, SE, SW
    0: ((0, 0), (1, 0), (1, 1), (0, 1)),
    1: ((0, 1), (0, 0), (1, 0), (1, 1)),
    2: ((1, 1), (0, 1), (0, 0), (1, 0)),
    3: ((1, 0), (1, 1), (0, 1), (0, 0)),
}


# --- GTA2 -------------------------------------------------------------------------------------------

G2_TILES = 8


def g2_index(tile, x, y):
    return (tile * 41 + x * 3 + y * 7) % 256


def g2_color(i, vpal):
    p = 65 if vpal % 2 else 2
    return ((255 - i + p) % 256, (i * 3 + p) % 256, (i + p) % 256, 255 if i else 0)


def g2_style():
    tile = bytearray(((G2_TILES + 3) // 4) * 16384)
    for t in range(G2_TILES):
        for y in range(64):
            for x in range(64):
                tile[(t // 4) * 16384 + y * 256 + (t % 4) * 64 + x] = g2_index(t, x, y)
    pp = bytearray(131072)
    for p in (2, 65):
        for c in range(256):
            off = (p // 64) * 65536 + c * 256 + (p % 64) * 4
            pp[off:off + 4] = bytes([(c + p) % 256, (c * 3 + p) % 256, (255 - c + p) % 256, 0])
    index = struct.pack('<16384H', *[65 if v % 2 else 2 for v in range(16384)])
    chunks = [(b'TILE', tile), (b'PPAL', pp), (b'PALX', index), (b'PALB', struct.pack('<8H', 8, 0, 0, 0, 0, 0, 0, 0)),
              (b'SPRG', b''), (b'SPRX', b'')]
    return b'GBST' + struct.pack('<H', 700) + b''.join(k + struct.pack('<I', len(v)) + v for k, v in chunks)


def g2_rgba(tile):
    return [g2_color(g2_index(tile, x, y), tile) for y in range(64) for x in range(64)]


def lid_word(tile, rot=0, flip=False, flat=False, light=0):
    return tile | (light << 10) | (0x1000 if flat else 0) | (0x2000 if flip else 0) | (rot << 14)


# (x, y, z, lid word, slope code)
G2_BLOCKS = [
    (5, 5, 0, lid_word(1), 0),
    (6, 5, 0, lid_word(2, rot=1), 0),
    (7, 5, 0, lid_word(3, rot=2, flip=True), 0),
    (8, 5, 0, lid_word(4, rot=3, flip=True, light=2), 0),
    (5, 6, 1, lid_word(1), 45),  # diagonal, facing up left
    (6, 6, 1, lid_word(2), 53),  # partial block left
    (7, 6, 0, lid_word(1), 0),
    (7, 6, 1, lid_word(3, flat=True), 0),
    (8, 6, 1, lid_word(5, rot=1), 61),  # partial centre block
    (5, 7, 1, lid_word(6), 50),  # 4-sided diagonal slope, facing up right
    (6, 7, 1, 1023, 49),  # 3-sided diagonal slope: dummy lid, no lid drawn
    (7, 7, 1, lid_word(7), 1),  # slope: up 26 low
    (8, 7, 0, lid_word(995), 0),  # reserved tile number: not drawn
]


def g2_gmp():
    blocks = [bytes(12)]
    cols = {}
    for x, y, z, lid, slope in G2_BLOCKS:
        ground = 1 if slope == 0 else 0
        blocks.append(struct.pack('<5HBB', 0, 0, 0, 0, lid, 0, (slope << 2) | ground))
        cols.setdefault((x, y), {})[z] = len(blocks) - 1
    column = [0]  # dword 0: the empty column (height 0, offset 0)
    base = [0] * 65536
    for (x, y), zs in sorted(cols.items()):
        offset, height = min(zs), max(zs) + 1
        base[y * 256 + x] = len(column)
        column.append(height | (offset << 8))
        column += [zs.get(z, 0) for z in range(offset, height)]
    dmap = struct.pack('<65536I', *base) + struct.pack('<I', len(column)) + struct.pack(f'<{len(column)}I', *column)
    dmap += struct.pack('<I', len(blocks)) + b''.join(blocks)
    zone = struct.pack('<6B', 1, 5, 5, 4, 3, 8) + b'Downtown'
    zone += struct.pack('<6B', 14, 0, 0, 20, 20, 8) + b'Zaibatsu'
    mobj = struct.pack('<HHBB', (6 << 7) | 64, (5 << 7) | 32, 64, 2)
    lght = struct.pack('<I4HBBBB', 0x00FF8000, (6 << 7), (6 << 7), (2 << 7), (3 << 7), 200, 0, 10, 5)
    anim = struct.pack('<HBBBB2H', 3, 4, 0, 2, 0, 3, 4)
    chunks = [(b'ZONE', zone), (b'DMAP', dmap), (b'MOBJ', mobj), (b'PSXM', bytes(2048)), (b'LGHT', lght),
              (b'ANIM', anim), (b'JUNK', b'unknown chunks are kept')]
    return b'GBMP' + struct.pack('<H', 500) + b''.join(k + struct.pack('<I', len(v)) + v for k, v in chunks)


# gta2_re @ edb484d3 MapRenderer.cpp: kLidFlags_620FA4 (indexed by lid >> 13) and draw_4F3FB0 give
# the texel corner at each map corner NW, NE, SE, SW for (rotation, flip):
G2_CORNERS = {
    (0, False): ((0, 0), (1, 0), (1, 1), (0, 1)),
    (0, True): ((1, 0), (0, 0), (0, 1), (1, 1)),
    (1, False): ((0, 1), (0, 0), (1, 0), (1, 1)),
    (1, True): ((0, 0), (0, 1), (1, 1), (1, 0)),
    (2, False): ((1, 1), (0, 1), (0, 0), (1, 0)),
    (2, True): ((0, 1), (1, 1), (1, 0), (0, 0)),
    (3, False): ((1, 0), (1, 1), (0, 1), (0, 0)),
    (3, True): ((1, 1), (1, 0), (0, 0), (0, 1)),
}


def texel(corners, fx, fy):
    """The texel (0..63) shown at fractional block position (fx, fy), from the corner table."""
    nw, ne, se, sw = corners
    u = nw[0] * (1 - fx) * (1 - fy) + ne[0] * fx * (1 - fy) + se[0] * fx * fy + sw[0] * (1 - fx) * fy
    v = nw[1] * (1 - fx) * (1 - fy) + ne[1] * fx * (1 - fy) + se[1] * fx * fy + sw[1] * (1 - fx) * fy
    return min(63, int(u * 64)), min(63, int(v * 64))


def inside(shape, px, py):
    """Whether pixel (px, py) of a block is covered by the lid of this shape (gta2_re lid extents)."""
    if shape == 'square':
        return True
    if shape in ('tri-NE-SE-SW', 'tri-NW-SE-SW'):
        if shape == 'tri-NE-SE-SW':
            return px + py >= 63  # the lower-right triangle
        return py >= px  # the lower-left triangle
    lo, hi = shape
    return lo[0] <= px < hi[0] and lo[1] <= py < hi[1]


def g2_shape(slope):
    if slope in (45, 49):
        return 'tri-NE-SE-SW'
    if slope in (46, 50):
        return 'tri-NW-SE-SW'
    if slope == 53:
        return ((0, 0), (24, 64))
    if slope == 61:
        return ((24, 24), (40, 40))
    return 'square'


def compose(region, draws):
    """RGBA image of `region` (x0, y0, x1, y1 blocks) from draws (bx, by, pixel(px, py) -> rgba|None,
    flat). Later draws cover earlier ones; transparent texels of flat lids let lower ones show."""
    x0, y0, x1, y1 = region
    w, h = (x1 - x0) * 64, (y1 - y0) * 64
    img = [(0, 0, 0, 0)] * (w * h)
    for bx, by, pixel, flat in draws:
        for py in range(64):
            for px in range(64):
                c = pixel(px, py)
                if c is None:
                    continue
                if c[3] == 0:
                    if flat:
                        continue
                    c = (c[0], c[1], c[2], 255)
                img[((by - y0) * 64 + py) * w + (bx - x0) * 64 + px] = c
    return w, h, bytes(v for p in img for v in p)


def g1_reference():
    draws = []
    for x, y, stack in G1_COLUMNS:
        for lid, rot, remap, flat in stack:
            if lid == 0:
                continue
            tex = g1_rgba(lid, remap)
            corners = G1_CORNERS[rot]

            def pixel(px, py, tex=tex, corners=corners):
                u, v = texel(corners, (px + 0.5) / 64, (py + 0.5) / 64)
                return tex[v * 64 + u]
            draws.append((x, y, pixel, flat))
    return compose((10, 20, 14, 22), draws)


def g2_reference():
    draws = []
    for x, y, z, lid, slope in sorted(G2_BLOCKS, key=lambda b: b[2]):
        tile = lid & 0x3FF
        if tile == 0 or tile >= 992 or slope in (49, 50, 51, 52) and tile == 1023:
            continue
        rot, flip, flat = lid >> 14, bool(lid & 0x2000), bool(lid & 0x1000)
        tex, corners, shape = g2_rgba(tile), G2_CORNERS[(rot, flip)], g2_shape(slope)

        def pixel(px, py, tex=tex, corners=corners, shape=shape):
            if not inside(shape, px, py):
                return None
            u, v = texel(corners, (px + 0.5) / 64, (py + 0.5) / 64)
            return tex[v * 64 + u]
        draws.append((x, y, pixel, flat))
    return compose((5, 5, 9, 8), draws)


# --- Radar ------------------------------------------------------------------------------------------

def radar_txd(k, size):
    # One D3D8 C8888 native (BGRA texels), as make_fixtures.py d3d8_native writes them.
    ver = 0x0C02FFFF if size == 8 else 0x1803FFFF
    texels = bytes(c for y in range(size) for x in range(size) for c in (y * 16 % 256, x * 16 % 256, k * 4 % 256, 255))
    s = struct.pack('<II', 8, 0x1102) + name32(f'radar{k:02d}') + name32('')
    s += struct.pack('<IIHHBBBB', 0x500, 1, size, size, 32, 1, 4, 0)
    s += struct.pack('<I', len(texels)) + texels
    native = chunk(0x15, chunk(1, s, ver) + chunk(3, b'', ver), ver)
    return txd([native], 1, ver)


def radar_reference(n, size):
    w = n * size
    img = bytearray(w * w * 4)
    for k in range(n * n):
        tx, ty = k % n, k // n
        for y in range(size):
            for x in range(size):
                o = ((ty * size + y) * w + tx * size + x) * 4
                img[o:o + 4] = bytes([k * 4 % 256, x * 16 % 256, y * 16 % 256, 255])
    return w, w, bytes(img)


def main():
    out = sys.argv[1]
    write(os.path.join(out, 'gta1/GTADATA/STYLE001.GRY'), g1_style())
    write(os.path.join(out, 'gta1/GTADATA/NYC.CMP'), g1_cmp())
    write(os.path.join(out, 'gta2/data/wil.sty'), g2_style())
    write(os.path.join(out, 'gta2/data/wil.gmp'), g2_gmp())
    write(os.path.join(out, 'gta2/data/test.seq'),
          b'MainOrBonus = MAIN\r\nGMPFile = data\\wil.gmp\r\nSTYFile = data\\wil.sty\r\nSCRFile = data\\wil.scr\r\n'
          b'Description = Downtown\r\n')
    d, img = img_v1([(f'radar{k:02d}.txd', radar_txd(k, 8)) for k in range(64)] + [('other.dff', b'not a model')])
    write(os.path.join(out, 'gta3/models/gta3.dir'), d)
    write(os.path.join(out, 'gta3/models/gta3.img'), img)
    write(os.path.join(out, 'sa/models/gta3.img'), img_v2([(f'radar{k:02d}.txd', radar_txd(k, 4)) for k in range(144)]))
    ref = {}
    for name, (w, h, px) in (('gta1_topdown', g1_reference()), ('gta2_topdown', g2_reference()),
                             ('gta3_radar', radar_reference(8, 8)), ('sa_radar', radar_reference(12, 4))):
        ref[name] = dict(width=w, height=h, fnv=fnv(px))
    ref['gta1'] = dict(objects=1, cars=1, routes=1, nav=['Downtown', 'Liberty'], police_stations=1)
    ref['gta2'] = dict(zones=[[1, 'Downtown'], [14, 'Zaibatsu']], objects=1, lights=1, animations=1,
                       chunks=['ANIM', 'DMAP', 'JUNK', 'LGHT', 'MOBJ', 'PSXM', 'ZONE'])
    open(os.path.join(out, 'map_reference.json'), 'w').write(json.dumps(ref, indent=2) + '\n')


if __name__ == '__main__':
    main()
