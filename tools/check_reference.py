#!/usr/bin/env python3
"""Checks an export of fixtures/gta3_pc against the hashes in reference.json (used by CI to make
sure the reference C ripper and the stored hashes still agree).

Usage: check_reference.py <export dir> <reference.json>"""
import hashlib, json, os, struct, sys

def data_chunk(p):
    d = open(p, 'rb').read(); pos = 12
    while pos + 8 <= len(d):
        n = struct.unpack('<I', d[pos + 4:pos + 8])[0]
        if d[pos:pos + 4] == b'data':
            return d[pos + 8:pos + 8 + n]
        pos += 8 + n + (n & 1)
    raise SystemExit(f'{p}: no data chunk')

root, ref = sys.argv[1], json.load(open(sys.argv[2]))
bad = [rel for rel, want in ref['gta3_pc'].items() if hashlib.sha256(data_chunk(os.path.join(root, rel))).hexdigest() != want]
if bad:
    raise SystemExit(f'{len(bad)} files differ from reference.json, e.g. {bad[:3]}')
print(f"{len(ref['gta3_pc'])} files match reference.json")
