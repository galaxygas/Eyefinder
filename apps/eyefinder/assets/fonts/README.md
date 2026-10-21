# Fonts

`Inter-Regular.ttf` and `Inter-SemiBold.ttf` are derived from the Inter variable font
(`Inter[opsz,wght].ttf`, Google Fonts repository, `ofl/inter`), licensed under the SIL Open Font
License 1.1 (`OFL.txt`). The Inter license names no Reserved Font Name.

They were produced with fontTools:

```
fonttools varLib.instancer "Inter[opsz,wght].ttf" wght=400 opsz=14 -o Inter-Regular.ttf --update-name-table
fonttools varLib.instancer "Inter[opsz,wght].ttf" wght=600 opsz=14 -o Inter-SemiBold.ttf --update-name-table
pyftsubset Inter-<weight>.ttf --unicodes="U+0000-DFFF,U+F900-FFFF,U+10000-10FFFF" --layout-features='*' \
    --glyph-names --name-IDs='*' --name-languages='*' --notdef-outline
```

The subset step removes Inter's Private Use Area glyphs so that they cannot shadow the Phosphor
icon font, which lives in that range.
