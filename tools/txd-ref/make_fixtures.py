#!/usr/bin/env python3
"""Synthetic texture fixtures for crates/ef-texture/tests (no game data).

    python3 tools/txd-ref/make_fixtures.py <txdref binary> <out dir>

* PC (D3D8, D3D9) and Xbox TXDs are written here from the documented layout
  (librw src/d3d/d3d8.cpp, d3d9.cpp, xbox.cpp `readNativeTexture`), with random texel data.
* PS2 TXDs are written by librw's own PS2 raster code (`txdref ps2gen`) for the three
  RenderWare versions the games use (gtamods "RenderWare" version table).
* IMG archives (v1 .dir/.img, v2 VER2) are written from the documented layout
  (re3 src/core/Directory.h, gta-reversed Directory.h) and contain some of the TXDs above.

Run make_reference.py afterwards to produce reference.json.
"""
import os
import random
import struct
import subprocess
import sys

STAMP = {  # library ID stamps: gtamods "RenderWare_Version", librw rwbase.h libraryIDPack
    "3.3.0.2": 0x0C02FFFF,
    "3.4.0.3": 0x1003FFFF,
    "3.5.0.0": 0x1400FFFF,
    "3.6.0.3": 0x1803FFFF,
}

C1555, C565, C4444, LUM8, C8888, C888, C555 = 0x100, 0x200, 0x300, 0x400, 0x500, 0x600, 0xA00
PAL8, PAL4, MIPMAP = 0x2000, 0x4000, 0x8000
BPP = {C1555: 2, C565: 2, C4444: 2, LUM8: 1, C8888: 4, C888: 4, C555: 2}
D3DFMT = {C1555: 25, C565: 23, C4444: 26, LUM8: 50, C8888: 21, C888: 22, C555: 24}  # D3DFORMAT values


def chunk(t, payload, ver):
    return struct.pack("<III", t, len(payload), ver) + payload


def levels_of(w, h, n):
    out = []
    for _ in range(n):
        out.append((w, h))
        w, h = max(1, w // 2), max(1, h // 2)
    return out


def level_size(fmt, dxt, w, h):
    if dxt:
        return ((w + 3) // 4) * ((h + 3) // 4) * (8 if dxt == 1 else 16)
    if fmt & (PAL8 | PAL4):
        return w * h  # one byte per texel, even for PAL4 (librw d3d.cpp rasterSetFormat)
    return w * h * BPP[fmt & 0xF00]


def texels(rng, fmt, dxt, w, h):
    n = level_size(fmt, dxt, w, h)
    if fmt & PAL4 and not dxt:
        return bytes(rng.randrange(16) for _ in range(n))
    return bytes(rng.randrange(256) for _ in range(n))


def palette(rng, fmt):
    if fmt & PAL8:
        return bytes(rng.randrange(256) for _ in range(1024))
    if fmt & PAL4:
        return bytes(rng.randrange(256) for _ in range(128))
    return b""


def name32(s):
    return s.encode().ljust(32, b"\0")


def d3d8_native(rng, name, fmt, w, h, nlev, dxt, has_alpha, ver):
    lv = levels_of(w, h, nlev)
    depth = 16 if dxt else (8 if fmt & (PAL8 | PAL4) else BPP[fmt & 0xF00] * 8)
    s = struct.pack("<II", 8, 0x1102) + name32(name) + name32("")
    s += struct.pack("<IIHHBBBB", fmt, has_alpha, w, h, depth, nlev, 4, dxt)
    s += palette(rng, fmt)
    for lw, lh in lv:
        t = texels(rng, fmt, dxt, lw, lh)
        s += struct.pack("<I", len(t)) + t
    return chunk(0x15, chunk(1, s, ver) + chunk(3, b"", ver), ver)


def d3d9_native(rng, name, fmt, w, h, nlev, dxt, has_alpha, ver):
    lv = levels_of(w, h, nlev)
    depth = 16 if dxt else (8 if fmt & (PAL8 | PAL4) else BPP[fmt & 0xF00] * 8)
    if dxt:
        d3dformat = struct.unpack("<I", b"DXT" + str(dxt).encode())[0]
        flags = 8 | (1 if has_alpha else 0)
    else:
        d3dformat = 41 if fmt & (PAL8 | PAL4) else D3DFMT[fmt & 0xF00]  # 41 = D3DFMT_P8
        flags = 1 if has_alpha else 0
    s = struct.pack("<II", 9, 0x1102) + name32(name) + name32("")
    s += struct.pack("<IIHHBBBB", fmt, d3dformat, w, h, depth, nlev, 4, flags)
    s += palette(rng, fmt)
    for lw, lh in lv:
        t = texels(rng, fmt, dxt, lw, lh)
        s += struct.pack("<I", len(t)) + t
    return chunk(0x15, chunk(1, s, ver) + chunk(3, b"", ver), ver)


XBOX_DXT = {1: 0x0C, 3: 0x0E, 5: 0x0F}  # librw src/d3d/rwxbox.h D3DFMT_DXT1/DXT3/DXT5


def xbox_native(rng, name, fmt, w, h, nlev, dxt, has_alpha, ver):
    lv = levels_of(w, h, nlev)
    depth = 16 if dxt else (8 if fmt & (PAL8 | PAL4) else BPP[fmt & 0xF00] * 8)
    data = b"".join(texels(rng, fmt, dxt, lw, lh) for lw, lh in lv)
    total = (len(data) + 3) & ~3
    s = struct.pack("<II", 5, 0x1102) + name32(name) + name32("")
    s += struct.pack("<IhhHHBBBBI", fmt, 1 if has_alpha else 0, 0, w, h, depth, nlev, 4, XBOX_DXT.get(dxt, 0), total)
    s += palette(rng, fmt) + data.ljust(total, b"\0")
    return chunk(0x15, chunk(1, s, ver) + chunk(3, b"", ver), ver)


def txd(natives, device, ver):
    body = chunk(1, struct.pack("<HH", len(natives), device), ver) + b"".join(natives) + chunk(3, b"", ver)
    return chunk(0x16, body, ver)


# (name, raster format, width, height, levels, dxt, hasAlpha)
PC_CASES = [
    ("c1555", C1555, 32, 16, 1, 0, 1),
    ("c565", C565, 32, 16, 1, 0, 0),
    ("c4444", C4444, 16, 16, 1, 0, 1),
    ("lum8", LUM8, 16, 8, 1, 0, 0),
    ("c8888", C8888, 16, 16, 1, 0, 1),
    ("c888", C888, 16, 16, 1, 0, 0),
    ("c555", C555, 16, 16, 1, 0, 0),
    ("c8888_mip", C8888 | MIPMAP, 64, 32, 7, 0, 1),
    ("pal8", PAL8 | C8888, 32, 32, 1, 0, 1),
    ("pal8_mip", PAL8 | C8888 | MIPMAP, 32, 16, 6, 0, 1),
    ("pal4", PAL4 | C8888, 32, 32, 1, 0, 1),
    ("dxt1_565", C565, 64, 64, 1, 1, 0),
    ("dxt1_1555", C1555, 32, 32, 1, 1, 1),
    ("dxt1_mip", C565 | MIPMAP, 64, 16, 7, 1, 0),
    ("dxt3", C4444, 32, 32, 1, 3, 1),
    ("dxt5", C8888, 32, 64, 1, 5, 1),
    ("dxt5_mip", C8888 | MIPMAP, 32, 32, 6, 5, 1),
    ("dxt2_unsupported", C8888, 16, 16, 1, 2, 1),
]

XBOX_CASES = [
    ("c1555", C1555, 32, 16, 1, 0, 1),
    ("c555", C555, 16, 16, 1, 0, 0),
    ("c8888", C8888, 64, 16, 1, 0, 1),
    ("c8888_sq", C8888, 32, 32, 1, 0, 1),
    ("c8888_mip", C8888 | MIPMAP, 64, 32, 7, 0, 1),
    ("pal8", PAL8 | C8888, 32, 32, 1, 0, 1),
    ("pal8_tall", PAL8 | C8888, 16, 64, 1, 0, 1),
    ("pal4", PAL4 | C8888, 32, 32, 1, 0, 1),
    ("dxt1", C565, 64, 64, 1, 1, 0),
    ("dxt1_alpha", C1555, 32, 32, 1, 1, 1),
    ("dxt3", C4444, 32, 32, 1, 3, 1),
    ("dxt5", C8888, 32, 64, 1, 5, 1),
    ("dxt5_mip", C8888 | MIPMAP, 32, 32, 6, 5, 1),
    ("c565_unsupported", C565, 16, 16, 1, 0, 0),
]


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


def main():
    txdref, out = sys.argv[1], sys.argv[2]
    os.makedirs(out, exist_ok=True)
    rng = random.Random(0xEF7E)
    files = {}
    for label, maker, cases, ver, dev in [
        ("d3d8_iii", d3d8_native, PC_CASES, STAMP["3.3.0.2"], 1),
        ("d3d8_vc", d3d8_native, PC_CASES, STAMP["3.4.0.3"], 1),
        ("d3d9_sa", d3d9_native, PC_CASES, STAMP["3.6.0.3"], 2),
        ("xbox_vc", xbox_native, XBOX_CASES, STAMP["3.5.0.0"], 8),
        ("xbox_sa", xbox_native, XBOX_CASES, STAMP["3.6.0.3"], 8),
    ]:
        natives = [maker(rng, f"{label}_{c[0]}", *c[1:], ver) for c in cases]
        files[label] = txd(natives, dev, ver)
        with open(os.path.join(out, f"{label}.txd"), "wb") as f:
            f.write(files[label])
    for label, ver, seed in [("ps2_iii", "0x31000", 3), ("ps2_vc", "0x33002", 33), ("ps2_sa", "0x36003", 36)]:
        path = os.path.join(out, f"{label}.txd")
        subprocess.run([txdref, "ps2gen", path, ver, str(seed)], check=True)
        files[label] = open(path, "rb").read()

    entries = [("loadsc0.txd", files["d3d8_vc"]), ("readme.txt", b"not a texture\n"), ("PS2.TXD", files["ps2_vc"])]
    d, img = img_v1(entries)
    open(os.path.join(out, "gta3_v1.dir"), "wb").write(d)
    open(os.path.join(out, "gta3_v1.img"), "wb").write(img)
    open(os.path.join(out, "gta3_v2.img"), "wb").write(img_v2([("vehicle.txd", files["d3d9_sa"]), ("empty.dat", b"")]))


if __name__ == "__main__":
    main()

# Non-RenderWare styles use the same fixture regeneration entry point.
import style_fixtures
style_fixtures.generate(sys.argv[2])
style_fixtures.generate_sty(sys.argv[2])
style_fixtures.generate_fon(sys.argv[2])
