# TideTerm app icon

## UI action icons (2026-10-02)

All custom UI glyphs are independently drawn with QPainter on one 16-unit grid;
`cpp/chrome/icon_shapes*.cpp` owns their geometry. Straight strokes snap to physical
pixels, with bounded lazy caching for each size, DPR and theme color. The complete
74-symbol catalogue, Chinese labels and meanings live in `chrome-icons.tsv`.
The 2026-10-04 clarity pass uses integer physical stroke widths, pixel-aligned
filled edges and the destination window's DPR for label glyphs; see the
[architecture guide](../../../docs/wiki/Architecture.md).
Clipboard copies, cloned windows, pane zoom, text zoom, panel floating/docking,
highlighting and triggers have distinct symbols. Split directions match the real
Qt orientation (horizontal = left/right; vertical = top/bottom).

Generate dark/light contact sheets from the actual production geometry:

```powershell
pwsh native/scripts/preview_icons.ps1 # QT_DIR must be set
```

Output: `native/target/diagnostics/icons/icons-dark.png` and `icons-light.png`.
Each sheet shows 32px geometry and 14px glyphs at 100/125/150/200% DPR without
resampling. This developer utility is independent of the shipped application.
Windows-associated file-type icons and the TideTerm brand mark remain their
existing native/brand assets, matching the reference's use of system associations.

## Application brand mark

`build_icons.py` owns the geometry (Pillow required) and regenerates every
artifact in one pass, so the vector and the bitmaps cannot drift apart:

```
python native/scripts/build_icons.py
```

The glyph is a navy tile with a two-stroke terminal prompt whose underscore is a
single-crest tide wave. Flat solid colors only — no gradients, texture, light
patches, or shadows. The wave is deliberately one crest rather than two: at
16 and 24 px a double crest collapses into a blob.

| File | Use |
|------|-----|
| `tideterm-icon.svg` | vector source, matches the rendered bitmaps |
| `tideterm-icon.png` | 1024×1024 runtime icon on other platforms and release ZIP asset |
| `tideterm-icon.ico` | Windows runtime icon, with 16/24/32/48/64/128/256 px entries |
| `tideterm-icon.icns` | Future macOS app bundle |

The replaced `icon.*` placeholders and duplicate intermediate PNG were removed;
Git history retains the earlier designs. The Windows release ZIP includes the PNG and ICO. The running Qt window
loads the platform's icon from beside the executable; `cargo run` loads it from this folder.
Windows uses the multi-size ICO so small window/taskbar icons do not retain the
decoded 1024×1024 PNG (about 4 MiB).
The application icon engine reads the ICO directory first and decodes only the
entry needed for a window's size/DPI request. All seven sizes remain available;
unused large entries do not stay decoded. The generated ICO has one entry per size.
