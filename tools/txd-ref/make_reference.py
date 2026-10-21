#!/usr/bin/env python3
"""Builds crates/ef-texture/tests/fixtures/reference.json for the ef-texture tests.

    python3 tools/txd-ref/make_reference.py <txdref binary> <fixture dir>

Three independent sources go into the reference:

1. librw (`txdref dump`, tools/txd-ref/txdref.cpp) reads every TXD and reports the stored bytes of
   each mip level (PC, Xbox) or the texels after it unswizzles them (PS2). This checks layout parsing
   and PS2 unswizzling exactly.
2. This script decodes every texture to RGBA by following the specifications literally, in exact
   rational arithmetic, with one rule for every channel: value = stored / max, converted to 8 bits by
   rounding to nearest (Vulkan spec, "Conversion From Floating-Point to Normalized Fixed-Point";
   ties round up). Sources per format are listed in docs/FORMATS.md. The Rust decoder must match
   these RGBA bytes exactly.
3. librw's and Pillow's own RGBA output are compared with (2) as a sanity check. Both truncate
   instead of rounding, so they may differ by 1 per channel (2 for S3TC, where they truncate both
   the endpoint expansion and the interpolation) and never more, except where librw is
   known to differ from the specification (its D3D palette path reads indices two texels ahead, its
   DXT5 colour block uses the DXT1 three-colour mode, and its Xbox path takes the unused top bit of
   X1R5G5B5 as alpha). Those cases are skipped for librw only.
"""
import io
import json
import os
import struct
import subprocess
import sys
from fractions import Fraction as F

from PIL import Image

C1555, C565, C4444, LUM8, C8888, C888, C555 = 0x100, 0x200, 0x300, 0x400, 0x500, 0x600, 0xA00
PAL8, PAL4 = 0x2000, 0x4000


def fnv(b):
    h = 0xCBF29CE484222325
    for x in b:
        h = ((h ^ x) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return f"{h:016x}"


def q(v):
    """Exact value in [0, 1] -> 8-bit, round to nearest (ties up)."""
    v = min(max(F(v), F(0)), F(1)) * 255
    return int(v + F(1, 2))  # floor(v + 1/2)


def u16(b, i):
    return b[i] | b[i + 1] << 8


# ---- per-texel colour definitions -------------------------------------------------------------
# D3D formats: Microsoft D3DFORMAT documentation (A1R5G5B5, R5G6B5, A4R4G4B4, L8, A8R8G8B8,
# X8R8G8B8, X1R5G5B5; little-endian, alpha/red in the high bits). Palette entries are D3D
# PALETTEENTRY {peRed, peGreen, peBlue, peFlags}, peFlags used as alpha.
def d3d_texel(fmt, b, i):
    p = fmt & 0xF00
    if p == C1555:
        v = u16(b, i)
        return (q(F(v >> 10 & 31, 31)), q(F(v >> 5 & 31, 31)), q(F(v & 31, 31)), 255 if v >> 15 else 0)
    if p == C555:
        v = u16(b, i)
        return (q(F(v >> 10 & 31, 31)), q(F(v >> 5 & 31, 31)), q(F(v & 31, 31)), 255)
    if p == C565:
        v = u16(b, i)
        return (q(F(v >> 11 & 31, 31)), q(F(v >> 5 & 63, 63)), q(F(v & 31, 31)), 255)
    if p == C4444:
        v = u16(b, i)
        return (q(F(v >> 8 & 15, 15)), q(F(v >> 4 & 15, 15)), q(F(v & 15, 15)), q(F(v >> 12 & 15, 15)))
    if p == LUM8:
        return (b[i], b[i], b[i], 255)
    if p == C8888:
        return (b[i + 2], b[i + 1], b[i], b[i + 3])
    if p == C888:
        return (b[i + 2], b[i + 1], b[i], 255)
    raise ValueError(hex(fmt))


BPP = {C1555: 2, C555: 2, C565: 2, C4444: 2, LUM8: 1, C8888: 4, C888: 4}


# ---- S3TC (Khronos Data Format Specification 1.3, "S3TC Compressed Texture Image Formats") --------
def s3tc_block(dxt, blk, opaque_dxt1):
    """16 RGBA texels of one block, row-major."""
    cblk = blk if dxt == 1 else blk[8:]
    c0, c1 = u16(cblk, 0), u16(cblk, 2)

    def rgb(c):
        return (F(c >> 11 & 31, 31), F(c >> 5 & 63, 63), F(c & 31, 31))

    p0, p1 = rgb(c0), rgb(c1)
    four = c0 > c1 or dxt != 1  # BC2/BC3 always use the non-transparent encodings
    if four:
        pal = [p0, p1, tuple((2 * a + b) / 3 for a, b in zip(p0, p1)), tuple((a + 2 * b) / 3 for a, b in zip(p0, p1))]
        alpha_c = [1, 1, 1, 1]
    else:
        pal = [p0, p1, tuple((a + b) / 2 for a, b in zip(p0, p1)), (F(0), F(0), F(0))]
        alpha_c = [1, 1, 1, 0]
    bits = struct.unpack_from("<I", cblk, 4)[0]
    out = []
    if dxt == 5:
        a0, a1 = F(blk[0], 255), F(blk[1], 255)
        if blk[0] > blk[1]:
            alut = [a0, a1] + [((7 - k) * a0 + k * a1) / 7 for k in range(1, 7)]
        else:
            alut = [a0, a1] + [((5 - k) * a0 + k * a1) / 5 for k in range(1, 5)] + [F(0), F(1)]
        abits = int.from_bytes(blk[2:8], "little")
    for t in range(16):
        code = bits >> (2 * t) & 3
        r, g, b = pal[code]
        if dxt == 1:
            a = 1 if opaque_dxt1 else alpha_c[code]
        elif dxt == 3:
            a = F(int.from_bytes(blk[0:8], "little") >> (4 * t) & 15, 15)
        else:
            a = alut[abits >> (3 * t) & 7]
        out.append((q(r), q(g), q(b), q(a)))
    return out


def s3tc_image(dxt, data, w, h, opaque_dxt1):
    bw, bs = (w + 3) // 4, 8 if dxt == 1 else 16
    img = bytearray(w * h * 4)
    for by in range((h + 3) // 4):
        for bx in range(bw):
            o = bs * (bw * by + bx)
            texels = s3tc_block(dxt, data[o : o + bs], opaque_dxt1)
            for t, c in enumerate(texels):
                x, y = bx * 4 + t % 4, by * 4 + t // 4
                if x < w and y < h:
                    img[(y * w + x) * 4 : (y * w + x) * 4 + 4] = bytes(c)
    return bytes(img)


# ---- Xbox swizzle: texel (x, y) is stored at the index whose bits interleave x and y, starting
# with x, while both coordinates still have bits; the larger dimension supplies the rest
# (librw src/d3d/xbox.cpp unswizzle, here written from that definition rather than ported).
def morton(x, y, w, h):
    idx, bit, sx, sy = 0, 0, 1, 1
    while sx < w or sy < h:
        if sx < w:
            idx |= (1 if x & sx else 0) << bit
            bit += 1
            sx <<= 1
        if sy < h:
            idx |= (1 if y & sy else 0) << bit
            bit += 1
            sy <<= 1
    return idx


# ---- fixture parsing (the layout written by make_fixtures.py) ----------------------------------
def natives(data):
    out, pos = [], 12 + 12 + 4
    while pos < len(data):
        t, size, _ = struct.unpack_from("<III", data, pos)
        if t != 0x15:
            break
        s = pos + 12 + 12
        platform = struct.unpack_from("<I", data, s)[0]
        n = {"platform": platform, "name": data[s + 8 : s + 40].split(b"\0")[0].decode()}
        if platform in (8, 9):
            fmt, extra, w, h, depth, nlev, rtype, comp = struct.unpack_from("<IIHHBBBB", data, s + 72)
            dxt = comp if platform == 8 else (int(chr(extra >> 24)) if comp & 8 else 0)
            p = s + 88
        else:
            fmt, alpha, _u, w, h, depth, nlev, rtype, comp, total = struct.unpack_from("<IhhHHBBBBI", data, s + 72)
            dxt = {0x0C: 1, 0x0E: 3, 0x0F: 5}.get(comp, 0) if comp else 0
            extra = alpha
            p = s + 92
        pal = b""
        if fmt & PAL8:
            pal, p = data[p : p + 1024], p + 1024
        elif fmt & PAL4:
            pal, p = data[p : p + 128], p + 128
        if platform in (8, 9):
            lsize = struct.unpack_from("<I", data, p)[0]
            lvl = data[p + 4 : p + 4 + lsize]
        else:
            lvl = data[p:]
        n.update(fmt=fmt, w=w, h=h, dxt=dxt, comp=comp, extra=extra, pal=pal, lvl=lvl)
        out.append(n)
        pos += 12 + size
    return out


def pc_supported(n):
    """Which fixture textures Eyefinder is expected to decode (docs/FORMATS.md)."""
    p = n["fmt"] & 0xF00
    if n["dxt"] not in (0, 1, 3, 5):
        return False  # DXT2/DXT4 (premultiplied): not decoded
    if n["platform"] == 5 and not n["dxt"] and p not in (C1555, C555, C8888):
        return False  # librw has no Xbox conversion for 565/4444/LUM8/888 to check against
    return True


def decode_pc(n):
    fmt, w, h, lvl, pal = n["fmt"], n["w"], n["h"], n["lvl"], n["pal"]
    xbox = n["platform"] == 5
    if n["dxt"]:
        if xbox:
            opaque = n["extra"] == 0  # librw xbox.cpp: DXT1 without hasAlpha is opaque
        else:
            opaque = fmt & 0xF00 == C565  # librw d3d.cpp: DXT1 in a C565 raster is opaque
        return s3tc_image(n["dxt"], lvl, w, h, opaque)
    out = bytearray()
    for y in range(h):
        for x in range(w):
            i = morton(x, y, w, h) if xbox else y * w + x
            if fmt & (PAL8 | PAL4):
                e = lvl[i] * 4
                if xbox:  # Xbox palettes are BGRA (librw xbox.cpp rasterToImage)
                    out += bytes((pal[e + 2], pal[e + 1], pal[e], pal[e + 3]))
                else:
                    out += pal[e : e + 4]
            else:
                out += bytes(d3d_texel(fmt, lvl, i * BPP[fmt & 0xF00]))
    return bytes(out)


# ---- PS2: colours of the texels librw unswizzled. 32-bit colours: alpha 0x80 = 1.0 (librw
# ps2raster.cpp rasterToImage, a*255/128). 16-bit colours are A1B5G5R5 (red in the low bits).
def ps2_colour(fmt, b, i):
    if fmt & 0xF00 == C8888:
        return (b[i], b[i + 1], b[i + 2], q(F(b[i + 3], 128)))
    v = u16(b, i)
    return (q(F(v & 31, 31)), q(F(v >> 5 & 31, 31)), q(F(v >> 10 & 31, 31)), 255 if v >> 15 else 0)


def decode_ps2(t):
    fmt = t["format"]
    plane = bytes.fromhex(t["levels"][0]["plane"])
    if fmt & (PAL8 | PAL4):
        pal = bytes.fromhex(t["palette"])
        return b"".join(bytes(ps2_colour(fmt, pal, i * 4)) for i in plane)
    step = 4 if fmt & 0xF00 == C8888 else 2
    return b"".join(bytes(ps2_colour(fmt, plane, i)) for i in range(0, len(plane), step))


def ps2_supported(t):
    fmt = t["format"]
    if fmt & (PAL8 | PAL4):
        return fmt & 0xF00 == C8888  # 16-bit CLUTs: librw does not convert them (FORMATS.md)
    return fmt & 0xF00 in (C8888, C1555)


# ---- sanity comparisons ---------------------------------------------------------------------------
def max_diff(a, b):
    return max(abs(x - y) for x, y in zip(a, b)) if len(a) == len(b) else 999


def pillow_rgba(n):
    fmt, w, h, lvl, pal, dxt = n["fmt"], n["w"], n["h"], n["lvl"], n["pal"], n["dxt"]
    if fmt & (PAL8 | PAL4):
        im = Image.frombytes("P", (w, h), lvl)
        im.putpalette(pal[: (16 if fmt & PAL4 else 256) * 4], rawmode="RGBA")
        return im.convert("RGBA").tobytes()
    masks = {
        C1555: (16, 0x7C00, 0x03E0, 0x001F, 0x8000),
        C565: (16, 0xF800, 0x07E0, 0x001F, 0),
        C4444: (16, 0x0F00, 0x00F0, 0x000F, 0xF000),
        C8888: (32, 0xFF0000, 0xFF00, 0xFF, 0xFF000000),
        C888: (32, 0xFF0000, 0xFF00, 0xFF, 0),
        C555: (16, 0x7C00, 0x03E0, 0x001F, 0),
    }
    hdr = bytearray(128)
    struct.pack_into("<4sIIIIIII", hdr, 0, b"DDS ", 124, 0x1007, h, w, len(lvl), 0, 1)
    if dxt:
        struct.pack_into("<II4s", hdr, 76, 32, 4, b"DXT%d" % dxt)
    elif fmt & 0xF00 == LUM8:
        struct.pack_into("<IIIIIII", hdr, 76, 32, 0x20000, 0, 8, 0xFF, 0, 0)
    else:
        bits, r, g, b, a = masks[fmt & 0xF00]
        struct.pack_into("<IIIIIIII", hdr, 76, 32, 0x40 | (1 if a else 0), 0, bits, r, g, b, a)
    struct.pack_into("<I", hdr, 108, 0x1000)
    im = Image.open(io.BytesIO(bytes(hdr) + lvl))
    im.load()
    rgba = bytearray(im.convert("RGBA").tobytes())
    if dxt == 1 and fmt & 0xF00 == C565:
        rgba[3::4] = b"\xff" * (len(rgba) // 4)
    return bytes(rgba)


def main():
    txdref, d = sys.argv[1], sys.argv[2]
    ref = {
        "_provenance": {
            "fixtures": "tools/txd-ref/make_fixtures.py",
            "structure": "librw aap/librw@18532c2e13efbc1aa43f0b00b4459831f9414de0 via tools/txd-ref/txdref.cpp",
            "rgba": "tools/txd-ref/make_reference.py specification decoder (exact arithmetic, round to nearest)",
            "sanity": f"librw and Pillow {__import__('PIL').__version__} within 1 per channel (2 for S3TC)",
            "digest": "FNV-1a 64",
        }
    }
    sanity = {"pillow": 0, "librw": 0}
    for f in sorted(os.listdir(d)):
        if not f.endswith(".txd"):
            continue
        path = os.path.join(d, f)
        dump = subprocess.run([txdref, "dump", path], check=True, capture_output=True, text=True).stdout
        texs = [json.loads(line) for line in dump.splitlines()]
        if f.startswith("ps2"):
            for t in texs:
                t["supported"] = ps2_supported(t)
                if t["supported"]:
                    rgba = decode_ps2(t)
                    t["rgba_spec"] = fnv(rgba)
                    lr = bytes.fromhex(t.pop("rgba_hex")) if "rgba_hex" in t else None
                    if lr is not None:
                        if max_diff(rgba, lr) > 1:
                            sys.exit(f"{f} {t['name']}: librw differs by {max_diff(rgba, lr)}")
                        sanity["librw"] += 1
                t["levels"] = [{k: v for k, v in lv.items() if k != "plane"} for lv in t["levels"]]
                t.pop("palette", None)
                t.pop("rgba_hex", None)
        else:
            pcs = {n["name"]: n for n in natives(open(path, "rb").read())}
            for t in texs:
                n = pcs[t["name"]]
                t["supported"] = pc_supported(n)
                lr = bytes.fromhex(t.pop("rgba_hex")) if t.get("rgba_hex") else None
                t.pop("rgba_hex", None)
                if not t["supported"]:
                    continue
                rgba = decode_pc(n)
                t["rgba_spec"] = fnv(rgba)
                tol = 2 if n["dxt"] else 1
                if n["platform"] != 5:
                    pd = max_diff(rgba, pillow_rgba(n))
                    if pd > tol:
                        sys.exit(f"{f} {t['name']}: Pillow differs by {pd}")
                    sanity["pillow"] += 1
                librw_differs = (
                    (n["platform"] != 5 and n["fmt"] & (PAL8 | PAL4))
                    or n["dxt"] == 5
                    or (n["platform"] == 5 and not n["dxt"] and n["fmt"] & 0xF00 == C555)
                )
                if lr is not None and not librw_differs:
                    ld = max_diff(rgba, lr)
                    if ld > tol:
                        sys.exit(f"{f} {t['name']}: librw differs by {ld}")
                    sanity["librw"] += 1
        for t in texs:
            t.pop("rgba", None)
        ref[f] = texs
    print(f"sanity: within tolerance of Pillow on {sanity['pillow']} and of librw on {sanity['librw']} textures")
    with open(os.path.join(d, "reference.json"), "w") as out:
        json.dump(ref, out, indent=1)
        out.write("\n")


if __name__ == "__main__":
    main()

import style_fixtures
style_fixtures.reference(sys.argv[2])
style_fixtures.reference_sty(sys.argv[2])
style_fixtures.reference_fon(sys.argv[2])
