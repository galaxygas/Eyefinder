#!/usr/bin/env python3
"""Synthetic GTA2 Dreamcast GD-ROM fixture: a .gdi sheet, a raw high-density data track with an
IP.BIN (KallistiOS makeip field layout; the USA disc's product number, version and date as redump
disc 22119 shows them) and Sega Dreamcast .STR streams (vgmstream
src/meta/str_sega.c header), plus their reference decoded by vgmstream-cli.

    dc_fixtures.py <vgmstream-cli at f7f764155b227f683adbab25402fe8f2be0b2ec2> <fixture dir>

The streams hold random bytes, so every AICA nibble code and the step-size clamps are exercised.
No game data is used.
"""
import hashlib, json, os, random, struct, subprocess, sys, tempfile

VGMSTREAM_COMMIT = 'f7f764155b227f683adbab25402fe8f2be0b2ec2'
rng = random.Random(20261022)


def str_file(channels, blocks, interleave, rate, short):
    """A single-track 4-bit STR; data_size is `short` bytes less than the padded blocks."""
    h = bytearray(0x800)
    data = bytes(rng.getrandbits(8) for _ in range(blocks * channels * interleave))
    struct.pack_into('<IiIiiIi', h, 0, 1, rate, 4, interleave, blocks, len(data) - short, channels)
    h[0xD4:0xD4 + 38] = b'\0Sega Stream Asset Builder Revision 1.0'[:38]
    return bytes(h) + data


def iso(files, base):
    def record(name, sector, size, directory):
        r = bytearray(33 + len(name) + (1 - len(name) % 2))
        r[0] = len(r); r[2:6] = struct.pack('<I', sector); r[6:10] = struct.pack('>I', sector)
        r[10:14] = struct.pack('<I', size); r[14:18] = struct.pack('>I', size)
        r[25] = 2 if directory else 0; r[32] = len(name); r[33:33 + len(name)] = name
        return bytes(r)
    disc = bytearray(19 * 2048)
    ip = bytearray(b' ' * 0x100)
    ip[0:16] = b'SEGA SEGAKATANA '; ip[0x10:0x20] = b'SEGA ENTERPRISES'
    ip[0x40:0x47] = b'T42102N'; ip[0x4A:0x50] = b'V1.002'; ip[0x50:0x58] = b'20000425'; ip[0x80:0x84] = b'GTA2'
    disc[:len(ip)] = ip
    d = record(b'\0', base + 18, 2048, True) + record(b'\x01', base + 18, 2048, True)
    for name, data in files:
        start = len(disc) // 2048
        d += record(name.encode() + b';1', base + start, len(data), False)
        disc += data
        disc += bytes(-len(disc) % 2048)
    disc[18 * 2048:18 * 2048 + len(d)] = d
    p = 16 * 2048
    disc[p] = 1; disc[p + 1:p + 6] = b'CD001'; disc[p + 6] = 1
    disc[p + 128:p + 130] = struct.pack('<H', 2048)
    root = record(b'\0', base + 18, 2048, True); disc[p + 156:p + 156 + len(root)] = root
    disc[17 * 2048] = 255; disc[17 * 2048 + 1:17 * 2048 + 6] = b'CD001'
    return bytes(disc)


def mode1(plain):
    out = bytearray()
    for i in range(0, len(plain), 2048):
        s = bytearray(2352); s[1:11] = b'\xff' * 10; s[15] = 1
        s[16:16 + 2048] = plain[i:i + 2048]
        out += s
    return bytes(out)


def main():
    vgm, out = sys.argv[1], sys.argv[2]
    base = os.path.join(out, 'gta2_dc'); os.makedirs(base, exist_ok=True)
    streams = [('MONO.STR', str_file(1, 3, 0x800, 22050, 0x123)), ('STEREO.STR', str_file(2, 2, 0x800, 44100, 0))]
    track3 = mode1(iso(streams, 45000))
    open(os.path.join(base, 'track03.bin'), 'wb').write(track3)
    open(os.path.join(base, 'track01.bin'), 'wb').write(mode1(bytes(4 * 2048)))
    open(os.path.join(base, 'track02.raw'), 'wb').write(bytes(4 * 2352))
    open(os.path.join(base, 'disc.gdi'), 'w').write(
        '3\n1 0 4 2352 track01.bin 0\n2 450 0 2352 track02.raw 0\n3 45000 4 2352 track03.bin 0\n')
    ref = {'_provenance': f'vgmstream-cli commit {VGMSTREAM_COMMIT}: SHA-256 of the WAV data chunk'}
    with tempfile.TemporaryDirectory() as tmp:
        for name, data in streams:
            src = os.path.join(tmp, name.lower()); open(src, 'wb').write(data)
            wav = os.path.join(tmp, name + '.wav')
            subprocess.run([vgm, '-o', wav, src], check=True, stdout=subprocess.DEVNULL)
            w = open(wav, 'rb').read()
            pos, chunks = 12, {}
            while pos + 8 <= len(w):
                cid, size = w[pos:pos + 4], struct.unpack_from('<I', w, pos + 4)[0]
                chunks[cid] = w[pos + 8:pos + 8 + size]; pos += 8 + size + (size & 1)
            ch, rate = struct.unpack_from('<HI', chunks[b'fmt '], 2)
            ref[name] = dict(sha256=hashlib.sha256(chunks[b'data']).hexdigest(), frames=len(chunks[b'data']) // (2 * ch),
                             channels=ch, rate=rate)
    open(os.path.join(out, 'dc_reference.json'), 'w').write(json.dumps(ref, indent=2) + '\n')


if __name__ == '__main__':
    main()
