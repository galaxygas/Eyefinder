"""Synthetic Truevision TGA fixtures (TGA 2.0 specification) and their reference.

The images are written by Pillow where it can write the layout (true-colour, grey, colour-mapped,
RLE); their reference RGBA is Pillow's decoding (`Image.open(...).convert("RGBA")`), independent of
Eyefinder's. Pillow cannot write 16-bit TGA and reads it differently from the specification
(5-bit channels shifted, `v << 3`, rather than scaled, and its own attribute-bit handling), so the
hand-written 16-bit files get a reference computed here directly from the specification: channels
round(v * 255 / 31), the attribute bit as alpha only when the descriptor declares one, rows in the
stated origin order. No game data is used.

    tga_fixtures.py <fixture dir>     writes the .tga files and tga_reference.json
"""
import hashlib, io, json, os, struct, sys


def pattern(w, h, mode):
    from PIL import Image
    im = Image.new(mode, (w, h))
    px = im.load()
    for y in range(h):
        for x in range(w):
            r, g, b, a = (x * 37 + y * 11) % 256, (x * 5 + y * 53) % 256, (x * y * 7 + 13) % 256, (x * 29 + y * 3) % 256
            px[x, y] = {'RGB': (r, g, b), 'RGBA': (r, g, b, a), 'L': r, 'P': (x + y * w) % 200}[mode]
    if mode == 'P':
        im.putpalette([v for i in range(256) for v in ((i * 3) % 256, (i * 7) % 256, (255 - i) % 256)])
    return im


def header(kind, w, h, depth, desc, cmap=None):
    first, length, bits = cmap or (0, 0, 0)
    return struct.pack('<BBBHHBHHHHBB', 0, 1 if cmap else 0, kind, first, length, bits, 0, 0, w, h, depth, desc)


def generate(out):
    files = {}
    # Pillow-written: bottom-left origin by default, RLE with compression="tga_rle".
    for name, mode, kw in [
        ('rgb24.tga', 'RGB', {}),
        ('rgba32_rle.tga', 'RGBA', {'compression': 'tga_rle'}),
        ('grey8_rle.tga', 'L', {'compression': 'tga_rle'}),
        ('mapped8.tga', 'P', {}),
    ]:
        buf = io.BytesIO()
        pattern(13, 9, mode).save(buf, format='TGA', **kw)
        files[name] = buf.getvalue()
    # Hand-written: 16-bit ARGB1555, top-left origin, with an alpha bit.
    w, h = 7, 5
    px = b''.join(struct.pack('<H', ((x + y) % 2) << 15 | (x * 4 % 32) << 10 | (y * 6 % 32) << 5 | (x * y % 32))
                  for y in range(h) for x in range(w))
    files['argb1555_top.tga'] = header(2, w, h, 16, 0x21) + px
    semantic = {}
    c5 = lambda v: (v * 255 * 2 + 31) // 62
    rgba = bytearray()
    for y in range(h):  # top-left origin: file order is image order
        for x in range(w):
            v = struct.unpack_from('<H', px, (y * w + x) * 2)[0]
            rgba += bytes([c5(v >> 10 & 31), c5(v >> 5 & 31), c5(v & 31), 255 if v & 0x8000 else 0])
    semantic['argb1555_top.tga'] = (w, h, bytes(rgba))
    # Hand-written: colour map of 16-bit entries starting at index 4 (cmap_first), bottom-left.
    cmap = b''.join(struct.pack('<H', 0x8000 | (i * 3 % 32) << 10 | (i * 5 % 32) << 5 | (31 - i % 32)) for i in range(20))
    idx = bytes(4 + (x + y) % 20 for y in range(h) for x in range(w))
    files['mapped_c16.tga'] = header(1, w, h, 8, 0x00, (4, 20, 16)) + cmap + idx
    rgba = bytearray()
    for y in range(h):  # bottom-left origin: the last stored row is the top image row
        for x in range(w):
            i = idx[(h - 1 - y) * w + x] - 4
            v = struct.unpack_from('<H', cmap, i * 2)[0]
            rgba += bytes([c5(v >> 10 & 31), c5(v >> 5 & 31), c5(v & 31), 255])  # no attribute bits declared
    semantic['mapped_c16.tga'] = (w, h, bytes(rgba))
    for name, data in files.items():
        open(os.path.join(out, name), 'wb').write(data)
    return files, semantic


def reference(out, generated):
    from PIL import Image
    files, semantic = generated
    ref = {}
    for name in sorted(files):
        if name in semantic:
            w, h, rgba = semantic[name]
            ref[name] = dict(width=w, height=h, sha256=hashlib.sha256(rgba).hexdigest(), reference='specification')
            continue
        im = Image.open(os.path.join(out, name)).convert('RGBA')
        ref[name] = dict(width=im.width, height=im.height, sha256=hashlib.sha256(im.tobytes()).hexdigest(),
                         reference='Pillow ' + __import__('PIL').__version__)
    open(os.path.join(out, 'tga_reference.json'), 'w').write(json.dumps(ref, indent=2) + '\n')


if __name__ == '__main__':
    out = sys.argv[1]
    reference(out, generate(out))
