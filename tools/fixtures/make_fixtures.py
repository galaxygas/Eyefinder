#!/usr/bin/env python3
"""Builds the synthetic test fixtures in crates/ef-audio/tests/fixtures.

Every file is generated (ffmpeg, random data, hand-built headers following the layouts documented
in docs/FORMATS). No game data is used. Requires ffmpeg (libmp3lame, libvorbis, mp2, mpeg1video)
and genisoimage. Usage: make_fixtures.py <output dir>
"""
import math, os, random, shutil, struct, subprocess, sys

OUT = sys.argv[1]
rng = random.Random(20261021)

def ff(*args):
    subprocess.run(['ffmpeg', '-v', 'error', '-y', '-bitexact', *args], check=True)

def rbytes(n):
    return bytes(rng.getrandbits(8) for _ in range(n))

def gxt(names):
    keys = sorted([('FEA_FM%d' % i, n) for i, n in enumerate(names)])
    tdat = b''; tkey = b''
    for k, v in keys:
        tkey += struct.pack('<I', len(tdat)) + k.encode().ljust(8, b'\0')
        tdat += v.encode('utf-16-le') + b'\0\0'
    return b'TKEY' + struct.pack('<I', len(tkey)) + tkey + b'TDAT' + struct.pack('<I', len(tdat)) + tdat

def add_cue(path, cues):
    d = open(path, 'rb').read()
    cue = struct.pack('<I', len(cues)) + b''.join(struct.pack('<II4sIII', i, f, b'data', 0, 0, f) for i, f, _ in cues)
    adtl = b'adtl'
    for i, f, l in cues:
        t = struct.pack('<I', i) + l.encode() + b'\0'
        adtl += b'labl' + struct.pack('<I', len(t)) + t + (b'\0' if len(t) & 1 else b'')
    body = d[12:] + b'cue ' + struct.pack('<I', len(cue)) + cue + b'LIST' + struct.pack('<I', len(adtl)) + adtl
    open(path, 'wb').write(b'RIFF' + struct.pack('<I', len(body) + 4) + b'WAVE' + body)

# ---------------- GTA III PC ----------------
g = os.path.join(OUT, 'gta3_pc'); a = os.path.join(g, 'audio')
for d in [a, os.path.join(g, 'movies'), os.path.join(g, 'TEXT')]:
    os.makedirs(d, exist_ok=True)
noise = lambda r, d, seed, ch=2: ['-f', 'lavfi', '-i', f'anoisesrc=r={r}:d={d}:a=0.3:seed={seed}', '-ac', str(ch)]
ff(*noise(32000, 0.6, 1), '-c:a', 'adpcm_ima_wav', f'{a}/HEAD.wav')
ff(*noise(32000, 0.6, 2), '-c:a', 'adpcm_ima_wav', f'{a}/CLASS.wav')
add_cue(f'{a}/CLASS.wav', [(1, 0, 'Intro DJ'), (2, 6400, 'Song A'), (3, 12800, 'Song B')])
ff(*noise(32000, 0.4, 3), '-c:a', 'adpcm_ms', f'{a}/KJAH.wav')
for i, n in enumerate(['RISE', 'LIPS', 'GAME', 'MSX', 'FLASH', 'CHAT']):
    ff(*noise(32000, 0.2, 10 + i), '-c:a', 'pcm_s16le', f'{a}/{n}.wav')
ff(*noise(44100, 0.5, 4, 1), '-c:a', 'libmp3lame', '-b:a', '128k', f'{a}/JB.MP3')
ff(*noise(22050, 0.5, 5), '-c:a', 'libmp3lame', '-q:a', '2', f'{a}/L1_LG.MP3')
ff(*noise(22050, 0.3, 6, 1), '-c:a', 'adpcm_ima_wav', f'{a}/A1_SSO.WAV')
ff(*noise(11025, 0.2, 7, 1), '-c:a', 'pcm_u8', f'{a}/lib_a1.WAV')
ff(*noise(44100, 0.1, 8, 1), '-c:a', 'pcm_s16le', f'{a}/extra.wav')
ff('-f', 'lavfi', '-i', 'testsrc=s=64x48:r=25:d=0.6', *noise(44100, 0.6, 9, 1), '-c:v', 'mpeg1video', '-c:a', 'mp2', '-b:a', '128k', '-f', 'mpeg', f'{g}/movies/GTAtitles.mpg')
ff('-f', 'lavfi', '-i', 'testsrc=s=64x48:r=25:d=0.6', *noise(48000, 0.6, 10, 2), '-c:v', 'mpeg2video', '-c:a', 'mp2', '-b:a', '192k', '-f', 'vob', f'{g}/movies/Logo.mpg')
raw = b''; sdt = b''
for i in range(3032):
    n = rng.randint(2, 24) * 2
    sdt += struct.pack('<IIIIi', len(raw), n, rng.choice([22050, 32000, 44100]), 0 if i % 2 else 20, -1 if i % 2 else 100)
    raw += rbytes(n)
open(f'{a}/SFX.RAW', 'wb').write(raw); open(f'{a}/SFX.SDT', 'wb').write(sdt)
open(f'{g}/TEXT/AMERICAN.GXT', 'wb').write(gxt(['HEAD RADIO', 'DOUBLE CLEF FM', 'K-JAH', 'RISE FM', 'LIPS 106', 'GAME FM', 'MSX FM', 'FLASHBACK 95.6', 'CHATTERBOX FM']))

# ---------------- Vice City PC ----------------
v = os.path.join(OUT, 'vc_pc'); va = os.path.join(v, 'audio'); os.makedirs(va, exist_ok=True)
tmp = os.path.join(OUT, '_tmp'); os.makedirs(tmp, exist_ok=True)
for i, n in enumerate(['WILD', 'FLASH', 'KCHAT', 'FEVER', 'VROCK', 'VCPR', 'ESPANT', 'EMOTION', 'WAVE']):
    ff(*noise(44100, 0.4, 20 + i), '-c:a', 'libmp3lame', '-b:a', '160k', f'{tmp}/{n}.mp3')
    open(f'{va}/{n}.ADF', 'wb').write(bytes(b ^ 0x22 for b in open(f'{tmp}/{n}.mp3', 'rb').read()))
ff(*noise(32000, 0.3, 30, 1), '-c:a', 'libmp3lame', f'{va}/MISCOM.MP3')
ff(*noise(22050, 0.3, 31, 1), '-c:a', 'adpcm_ima_wav', f'{va}/MOBR1.WAV')
raw = b''; sdt = b''
for i in range(749):
    n = rng.randint(2, 20) * 2; sdt += struct.pack('<iIiii', len(raw), n, 22050, 0, -1); raw += rbytes(n)
open(f'{va}/SFX.RAW', 'wb').write(raw); open(f'{va}/SFX.SDT', 'wb').write(sdt)

# ---------------- San Andreas PC ----------------
s = os.path.join(OUT, 'sa_pc')
for d in ['audio/CONFIG', 'audio/STREAMS', 'audio/SFX']:
    os.makedirs(f'{s}/{d}', exist_ok=True)
KEY = bytes([0xEA, 0x3A, 0xC4, 0xA1, 0x9A, 0xA8, 0x14, 0xF3, 0x48, 0xB0, 0xD7, 0x23, 0x9D, 0xE8, 0xFF, 0xF1])
packs = ['AA', 'CH']
open(f'{s}/audio/CONFIG/STRMPAKS.DAT', 'wb').write(b''.join(p.encode().ljust(16, b'\0') for p in packs))
lk = b''
for pi, p in enumerate(packs):
    blob = bytearray()
    for t in range(2):
        ff(*noise(32000, 0.5 + 0.25 * t, 40 + 2 * pi + t), '-c:a', 'libvorbis', '-q:a', '3', f'{tmp}/{p}{t}.ogg')
        ogg = open(f'{tmp}/{p}{t}.ogg', 'rb').read()
        hdr = bytearray(0x1F84)
        for b in range(4):
            struct.pack_into('<II', hdr, b * 8, 500 * (b + 1), b)
        struct.pack_into('<I', hdr, 0x1F40, len(ogg)); struct.pack_into('<I', hdr, 0x1F44, 32000)
        off = len(blob); blob += hdr + ogg
        lk += struct.pack('<B3xII', pi, off, len(hdr) + len(ogg))
    open(f'{s}/audio/STREAMS/{p}', 'wb').write(bytes(b ^ KEY[i & 15] for i, b in enumerate(blob)))
    shutil.copy(f'{tmp}/{p}0.ogg', f'{OUT}/sa_{p}0_plain.ogg')
open(f'{s}/audio/CONFIG/TRAKLKUP.DAT', 'wb').write(lk)
open(f'{s}/audio/CONFIG/PAKFILES.DAT', 'wb').write(b'GENRL'.ljust(12, b'\0') + bytes(40) + b'SCRIPT'.ljust(12, b'\0') + bytes(40))
bl = b''
for pak, name in enumerate(['GENRL', 'SCRIPT']):
    blob = bytearray()
    for bank in range(2):
        sizes = [rng.randint(10, 120) * 2 for _ in range(3)]
        h = bytearray(0x12C4); struct.pack_into('<h', h, 0, 3); o = 0
        for i, sz in enumerate(sizes):
            struct.pack_into('<IIHh', h, 4 + i * 12, o, 0xFFFFFFFF, 22050 + 1000 * i, -100 * i); o += sz
        off = len(blob); blob += h + rbytes(o)
        bl += struct.pack('<B3xII', pak, off, o)
    open(f'{s}/audio/SFX/{name}', 'wb').write(bytes(blob))
open(f'{s}/audio/CONFIG/BANKLKUP.DAT', 'wb').write(bl)

# ---------------- GTA III PS2 (ISO) ----------------
F = [(0, 0), (60, 0), (115, -52), (98, -55), (122, -60)]
def enc_frame(samples, h1, h2):
    best = None
    for f in range(5):
        for sh in range(13):
            x, y = h1, h2; nib = []; err = 0
            for smp in samples:
                pred = (x * F[f][0] + y * F[f][1]) >> 6
                n = max(-8, min(7, round((smp - pred) / (1 << (12 - sh)))))
                o = max(-32768, min(32767, ((n << 12) >> sh) + pred))
                err += (smp - o) ** 2; nib.append(n & 15); y, x = x, o
            if best is None or err < best[0]:
                best = (err, f, sh, nib, x, y)
    _, f, sh, nib, x, y = best
    return bytes([f << 4 | sh, 0]) + bytes(nib[i] | nib[i + 1] << 4 for i in range(0, 28, 2)), x, y
def vb(path, blocks, kind):
    data = bytearray(); st = [(0, 0), (0, 0)]; t = 0
    for _ in range(blocks):
        for c in range(2):
            if kind == 'random':
                data += rbytes(0x2000); continue
            for line in range(0x2000 // 16):
                smp = [int(12000 * math.sin(2 * math.pi * (300 + 200 * c) * (t + i + line * 28) / 32000)) for i in range(28)]
                fr, x, y = enc_frame(smp, *st[c]); st[c] = (x, y); data += fr
        t += 0x2000 // 16 * 28
    open(path, 'wb').write(bytes(data))
p = os.path.join(tmp, 'ps2root'); os.makedirs(f'{p}/AUDIO/MUSIC', exist_ok=True); os.makedirs(f'{p}/TEXT', exist_ok=True)
vb(f'{p}/AUDIO/MUSIC/HEAD.VB', 2, 'tone')
vb(f'{p}/AUDIO/MUSIC/CHAT.VB', 2, 'random')
for n in ['CLASS', 'KJAH', 'RISE', 'LIPS', 'GAME', 'MSX', 'FLASH']:
    vb(f'{p}/AUDIO/MUSIC/{n}.VB', 1, 'random')
open(f'{p}/SYSTEM.CNF', 'w').write('BOOT2 = cdrom0:\\SLUS_200.62;1\r\nVER = 1.00\r\nVMODE = NTSC\r\n')
shutil.copy(f'{g}/TEXT/AMERICAN.GXT', f'{p}/TEXT/AMERICAN.GXT')
subprocess.run(['genisoimage', '-quiet', '-o', f'{OUT}/gta3_ps2.iso', '-V', 'GTA3', p], check=True)

# ---------------- noise streams for seek tests ----------------
ff(*noise(44100, 2.0, 50), '-c:a', 'libmp3lame', '-b:a', '128k', f'{OUT}/seek.mp3')
ff(*noise(32000, 2.0, 51), '-c:a', 'libvorbis', '-q:a', '3', f'{OUT}/seek.ogg')
shutil.rmtree(tmp)
print('fixtures written to', OUT)
