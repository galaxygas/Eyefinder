#!/usr/bin/env python3
"""Generate crates/ef-audio/src/tables/*.rs from the re3 / reVC reverse-engineered sources.

The re3 projects carry no license, so their code is never copied into Eyefinder.  This script only
extracts facts (the file names the game opens and the identifiers it uses for them) and writes them
as Rust data with a citation.  Usage:

    gen_tables.py <re3 src dir> <reVC src dir> <output dir>

Each table is built by running the C preprocessor over the original declarations with the same
defines as the build in question (PC: none, PS2: GTA_PS2 + PS2_AUDIO_PATHS), then pairing the
n-th file name with the n-th enum value, which is how the game indexes them.
"""
import os, re, subprocess, sys

def cpp(text, defines):
    args = ['gcc', '-E', '-P', '-x', 'c', '-'] + ['-D' + d for d in defines]
    return subprocess.run(args, input=text, capture_output=True, text=True, check=True).stdout

def array_block(src, decl_regex, nth=0):
    """Return the text of the nth array whose declaration matches decl_regex, up to its '};'."""
    lines = src.splitlines()
    starts = [i for i, l in enumerate(lines) if re.search(decl_regex, l)]
    i = starts[nth]
    out = []
    for l in lines[i:]:
        out.append(l)
        if l.strip().startswith('};'):
            break
    return '\n'.join(out)

def tables_region(src):
    """The region of sampman.h holding the conditional StreamedNameTable definitions."""
    a = src.index('#if defined(OPUS_AUDIO_PATHS)')
    # end: the closing #endif of that #if block
    depth, pos = 0, a
    for m in re.finditer(r'^\s*#\s*(if|ifdef|ifndef|endif)\b', src[a:], re.M):
        depth += 1 if m.group(1) != 'endif' else -1
        if depth == 0:
            end = a + m.end()
            # reVC: entries shared by every build follow the #endif and the array closes after them
            if src[end:].lstrip().startswith('"'):
                end = src.index('};', end) + 2
            return src[a:end]
    raise SystemExit('unterminated table region')

def files_from(region, defines, want_ps2):
    pp = cpp(region, defines)
    # re3 names its PS2 table PS2StreamedNameTable; reVC swaps StreamedNameTable itself under PS2_AUDIO_PATHS
    names = ['PS2StreamedNameTable', 'StreamedNameTable'] if want_ps2 else ['StreamedNameTable']
    for name in names:
        m = re.search(r'\b' + name + r'\s*\[\]\s*\[\d+\]\s*=\s*\{(.*?)\};', pp, re.S)
        if m:
            break
    return [s.replace('\\\\', '\\') for s in re.findall(r'"([^"]*)"', m.group(1))]

def enum_values(src, enum_name, stop, defines):
    body = src[src.index('enum ' + enum_name):]
    body = body[:body.index('};') + 2]
    pp = cpp(body, defines)
    inner = pp[pp.index('{') + 1:pp.index(stop)]
    return [v.split('=')[0].strip() for v in inner.split(',') if v.strip()]

III_RULES = [
    (lambda n, i: i <= 8, 'Radio'),
    (lambda n, i: n == 'STREAMED_SOUND_RADIO_MP3_PLAYER', None),
    (lambda n, i: n in ('STREAMED_SOUND_RADIO_POLICE', 'STREAMED_SOUND_CITY_AMBIENT', 'STREAMED_SOUND_WATER_AMBIENT'), 'PoliceAmbience'),
    (lambda n, i: n.startswith('STREAMED_SOUND_ANNOUNCE') or n in ('STREAMED_SOUND_NEWS_INTRO', 'STREAMED_SOUND_BANK_INTRO'), 'Announcements'),
    (lambda n, i: n.startswith('STREAMED_SOUND_CUTSCENE_'), 'Cutscenes'),
    (lambda n, i: n in ('STREAMED_SOUND_MISSION_COMPLETED', 'STREAMED_SOUND_GAME_COMPLETED'), 'MusicCues'),
    (lambda n, i: n.startswith('SFX_MISSION_'), 'MissionAudio'),
]
VC_RULES = [
    (lambda n, i: i <= 8, 'Radio'),
    (lambda n, i: n == 'STREAMED_SOUND_RADIO_MP3_PLAYER', None),
    (lambda n, i: n in ('STREAMED_SOUND_RADIO_POLICE', 'STREAMED_SOUND_RADIO_TAXI') or n.endswith('_AMBIENT') or n.startswith('STREAMED_SOUND_AMBSIL'), 'PoliceAmbience'),
    (lambda n, i: n.startswith('STREAMED_SOUND_ANNOUNCE'), 'Announcements'),
    (lambda n, i: n.startswith('STREAMED_SOUND_CUTSCENE_'), 'Cutscenes'),
    (lambda n, i: n.startswith('STREAMED_SOUND_MISSION_COMPLETED') or n == 'STREAMED_SOUND_GAME_COMPLETED', 'MusicCues'),
    (lambda n, i: n.startswith('STREAMED_SOUND_MISSION_'), 'MissionAudio'),
]

def categorize(pairs, rules, label):
    rows, unmapped = [], []
    for i, (f, n) in enumerate(pairs):
        for test, cat in rules:
            if test(n, i):
                break
        else:
            unmapped.append(n)
            continue
        if cat is not None:
            rows.append((f, n, cat))
    if unmapped:
        raise SystemExit(f'{label}: unmapped enum values {unmapped}')
    # Several ids can point at one file: reVC fills unused slots (leftover GTA III cutscene ids, extra
    # mission-complete ids) with MISCOM. Keep one row per file, preferring the MusicCues id because that
    # is the slot the game actually plays the file from; otherwise the first id wins.
    out, index = [], {}
    for f, n, c in rows:
        key = f.upper()
        if key not in index:
            index[key] = len(out)
            out.append((f, n, c))
        elif c == 'MusicCues' and out[index[key]][2] != 'MusicCues':
            out[index[key]] = (f, n, c)
    return out

def emit(path, const, rows, source):
    with open(path, 'w') as o:
        o.write('// @generated by tools/gen_tables.py - do not edit.\n')
        o.write(f'// Source: {source}\n')
        o.write('use crate::tables::{StreamDef, StreamCategory::*};\n\n')
        o.write(f'pub static {const}: &[StreamDef] = &[\n')
        for f, n, c in rows:
            f = f.replace('\\', '/')
            o.write(f'    StreamDef {{ path: "{f}", id: "{n}", category: {c} }},\n')
        o.write('];\n')

def emit_names(path, const, names, source):
    with open(path, 'w') as o:
        o.write('// @generated by tools/gen_tables.py - do not edit.\n')
        o.write(f'// Source: {source}\n\n')
        o.write(f'pub static {const}: &[&str] = &[\n')
        for n in names:
            o.write(f'    "{n}",\n')
        o.write('];\n')

def main():
    re3, revc, out = sys.argv[1:4]
    rd = lambda d, p: open(os.path.join(d, p), encoding='latin-1').read()
    os.makedirs(out, exist_ok=True)
    for game, d, rules, prefix in (('III', re3, III_RULES, 'III'), ('VC', revc, VC_RULES, 'VC')):
        samp = rd(d, 'audio/sampman.h')
        enums = rd(d, 'audio/audio_enums.h')
        region = tables_region(samp)
        for plat, defines, ps2 in (('PC', [], False), ('PS2', ['GTA_PS2', 'PS2_AUDIO_PATHS'], True)):
            files = files_from(region, defines, ps2)
            # re3/reVC index the PS2 path table with the PC enum (PS2_AUDIO_PATHS lets a PC build load PS2
            # files), so both tables pair with the PC enum.  Entries of the PS2 table that are not .VB point
            # back at PC files and are dropped: on the PS2 those sounds live in the SFX bank instead.
            names = enum_values(enums, 'eStreamedSounds', 'TOTAL_STREAMED_SOUNDS', [])
            if len(files) != len(names):
                raise SystemExit(f'{game} {plat}: {len(files)} files vs {len(names)} enum values')
            pairs = list(zip(files, names))
            if ps2:
                pairs = [(f, n) for f, n in pairs if f.upper().endswith('.VB')]
            rows = categorize(pairs, rules, f'{game} {plat}')
            src = f'{"re3" if game == "III" else "reVC"} src/audio/sampman.h ({"PS2StreamedNameTable" if ps2 else "StreamedNameTable"}, defines: {defines or "none"}) + audio_enums.h eStreamedSounds'
            emit(os.path.join(out, f'{prefix.lower()}_{plat.lower()}_streams.rs'), f'{prefix}_{plat}_STREAMS', rows, src)
            print(game, plat, len(files), 'entries ->', len(rows), 'unique files')
        sfx = enum_values(rd(d, 'audio/AudioSamples.h'), 'eSfxSample', 'TOTAL_AUDIO_SAMPLES', [])
        emit_names(os.path.join(out, f'{prefix.lower()}_pc_sfx_names.rs'), f'{prefix}_PC_SFX_NAMES', sfx,
                   f'{"re3" if game == "III" else "reVC"} src/audio/AudioSamples.h eSfxSample (PC build)')
        print(game, 'PC SFX names', len(sfx))

if __name__ == '__main__':
    main()
