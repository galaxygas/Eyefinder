#!/bin/sh
# Builds the 32-bit Windows plugin (.asi) and standalone tool (.exe) with zig's bundled MinGW toolchain,
# plus a native build for testing.  Requires: pip install ziglang
set -e
cd "$(dirname "$0")"
mkdir -p dist
ZIGCC="python3 -m ziglang cc"
FLAGS="-O2 -std=c99 -Wall -Wno-unused-function -target x86-windows-gnu"
$ZIGCC $FLAGS -shared -DRIPPER_ASI -o dist/gta3_music_ripper.asi src/ripper.c -lkernel32
$ZIGCC $FLAGS -o dist/gta3_music_ripper.exe src/ripper.c -lkernel32 -lshell32
rm -f dist/*.lib dist/*.pdb
cp gta3_music_ripper.ini dist/
if command -v gcc >/dev/null; then gcc -O2 -o dist/gta3_music_ripper_native src/ripper.c -lm; fi
echo "built: $(ls dist)"
