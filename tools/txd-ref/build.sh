#!/usr/bin/env bash
# Builds the librw reference tool and regenerates the ef-texture fixtures and reference.
#
#   tools/txd-ref/build.sh <work dir> [<fixture out dir>]
#
# Needs git, cmake, a C++ compiler, python3 and Pillow. librw (MIT, https://github.com/aap/librw)
# is fetched at the commit the committed reference was made with.
set -euo pipefail
LIBRW_COMMIT=18532c2e13efbc1aa43f0b00b4459831f9414de0
here="$(cd "$(dirname "$0")" && pwd)"
work="$1"
out="${2:-$work/fixtures}"
mkdir -p "$work"
if [[ ! -d "$work/librw" ]]; then
  git init -q "$work/librw"
  git -C "$work/librw" fetch -q --depth 1 https://github.com/aap/librw "$LIBRW_COMMIT"
  git -C "$work/librw" checkout -q FETCH_HEAD
fi
cmake -S "$work/librw" -B "$work/librw/build" -DLIBRW_PLATFORM=NULL -DLIBRW_TOOLS=OFF -DCMAKE_BUILD_TYPE=Release > /dev/null
cmake --build "$work/librw/build" --parallel > /dev/null
c++ -O1 -std=c++11 -DRW_NULL -I"$work/librw" "$here/txdref.cpp" "$work/librw/build/src/librw.a" -o "$work/txdref"
rm -rf "$out"
python3 "$here/make_fixtures.py" "$work/txdref" "$out"
python3 "$here/make_reference.py" "$work/txdref" "$out" 2> >(grep -v "Invalid raster format" >&2)
python3 "$here/tga_fixtures.py" "$out"
