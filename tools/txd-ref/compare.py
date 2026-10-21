#!/usr/bin/env python3
"""Compares regenerated texture fixtures with the committed ones (CI).

    python3 tools/txd-ref/compare.py <regenerated dir> <committed dir>

Every fixture file must be byte-identical and reference.json must agree apart from `_provenance`
(which records tool versions).
"""
import json
import os
import sys

new, old = sys.argv[1], sys.argv[2]
bad = 0
for f in sorted(os.listdir(old)):
    a, b = os.path.join(new, f), os.path.join(old, f)
    if f == "reference.json":
        ra, rb = json.load(open(a)), json.load(open(b))
        ra.pop("_provenance"), rb.pop("_provenance")
        same = ra == rb
    else:
        same = os.path.exists(a) and open(a, "rb").read() == open(b, "rb").read()
    print(("same     " if same else "DIFFERENT"), f)
    bad += not same
sys.exit(1 if bad else 0)
