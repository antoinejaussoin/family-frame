# Photo dither comparison

Use this when choosing (or re-choosing) the Spectra 6 photo recipe. The live knobs
live in `src/photo.rs`:

- `PHOTO_DITHER_MODE` — error-diffusion kernel (currently Burkes)
- `PHOTO_PUNCH` — shared sRGB contrast (pivot 0.5) and OKLab saturation (currently 0.90)
- `DITHER_VERSION` — bump this whenever either of the above changes so cached `.bin` / preview PNGs rebuild

The gold **now** cell in the generated grid tracks those two knobs, so a later re-run
highlights whatever is in production.

## Generate the grid

From `server/`, with the photos listed in `examples/dither_matrix.rs` (`PHOTOS`) present
under `pictures/<id>.jpg`:

```bash
cargo run --release --example dither_matrix
```

Writes 5 × 5 × 10 = 250 JPEG previews plus `index.html` to `out/dither-matrix/`
(gitignored). Takes about a minute. Edit `PHOTOS`, `MODES`, and `PUNCHES` in
`dither_matrix.rs` if the set of pictures or axes should change.

## Serve it

`file://` is awkward for this many images. From the output dir:

```bash
cd out/dither-matrix
python3 -m http.server 8766 --bind 127.0.0.1
```

Open http://127.0.0.1:8766/

Click a cell to zoom. Star (or `F`) builds a shortlist; copy it back into chat.

## Axes

| | |
|---|---|
| **X — punch** | Same multiplier for contrast and saturation: 0.55 … 1.80. `1.00` is identity on top of the fixed pipeline. |
| **Y — dither** | Atkinson, Floyd–Steinberg, Burkes, Jarvis–Judice–Ninke, Ordered (Bayer 4×4). |

Held fixed (match production): cover-crop 4:3 → 1600×1200, unsharp 0.6, exposure 1.05,
shadows 0.15, highlights 0.3, tone auto, gamut auto, green ink remapped to black.

## Apply a winner

1. Set `PHOTO_DITHER_MODE` and `PHOTO_PUNCH` in `src/photo.rs`.
2. Bump `DITHER_VERSION`.
3. Restart the server.
4. Hitting `/api/pictures/{id}/dither.png` (or loading a picture in the family UI)
   regenerates that photo’s cache when the stored version is stale.

Do not change the dashboard packer in `src/pack.rs` — that path is Floyd–Steinberg
for HTML, not photos.
