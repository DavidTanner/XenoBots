# Resolution-independent sprites (T-070)

How higher-resolution art can replace the original's images one at a time, at output
resolution, without changing what the game draws. This is the groundwork for the HD
asset pack (T-072) and sharp UI text (T-074). No HD art exists in the repository; a
pack is always the user's, loaded at run time.

## The rule: the indexed framebuffer stays the truth

Everything the game draws still goes into `xeno_render::Framebuffer`, 8-bit indices at
320x200 or 320x240, by the original's rules. The sim, replays, tests and the pointer
probe (`docs/render.md`, "The pointer probe (T-133)") only ever see those pixels. HD art
is a presentation layer on top: it changes what a window shows, never a framebuffer
pixel, so `xeno-app` with or without a pack plays exactly the same game.

## Three pieces

```
 game drawing ──► Framebuffer (indices) ──► present ──► window words (0x00RRGGBB)
        │                 ▲                                ▲
        └─► Recording ────┘ owners per pixel               │
              blits: [BlitRecord] ──► compose ─────────────┘ HD texels over owned pixels
                                       ▲
                          HashMap<AssetId, HdImage> (T-072 fills it)
```

1. **Names** (`xeno_render::hd::AssetId`). Every image the original draws gets a name
   that does not depend on where or how big it is drawn:
   - `Sprite { bank, index }`: sprite `index` of `msprite{bank}.b@n`, numbered through
     the banks' one directory (`docs/formats/b@n.md`).
   - `ModelImage { model, view, image }`: image `image` of view `view` of a model,
     views numbered along the loaded chain (`.m08` first, then `.m16`, `.m32`, as
     `LoadedModel` keeps them), or of its `.m00` when `view` is `None`. `model` is the
     file's base name (`hd::Name`, 8 bytes, lower case; `LoadedModel::name`, set by
     the app's loaders). A frame is a list of these images placed at offsets
     (`docs/formats/mxx.md`), and frames share images, so a pack replaces images, not
     frames: one HD image fixes every frame and facing that uses it.
   - `File(name)`: a whole file drawn as one picture: a `.m@l` screen, a `.vcl`
     picture, a sky strip.

   Glyphs for T-074 would add a `Glyph { font, code }` name the same way.

2. **The record** (`Framebuffer::set_recording`, `hd::Recording`). Off by default.
   When on, each named blit pushes a `BlitRecord`: its name, the **logical rectangle
   the whole image covers** in surface pixels before clipping, its 8.8 scale and
   whether it was mirrored or flipped. For a model image the rectangle is the part's
   unit rectangle scaled from the anchor by the repeat table (`ModelScale::start` and
   `count`; `mxx.md`, "Drawing"), which is why the record keeps the rectangle and the
   scale and not just a screen position (the note from T-029): the original scales a
   unit by repeating or dropping whole pixels unevenly, while an HD image is stretched
   evenly over the same rectangle.

   Alongside the pixels the record keeps an **owner** word per surface pixel:
   `(k + 1) << 8 | colour` for the blit `k` that last wrote it and the colour it
   wrote. A pixel still belongs to blit `k` only while the framebuffer still holds that
   colour there, so anything drawn over it later (another blit, text, a polygon, a
   copied picture) takes it away without having to know about the record. `clear` and
   `fill_rect` reset the owners they cover. The known gap: a later write of the
   *same* colour is not seen (a `shortcut:` comment in `hd.rs`); it only matters if a
   pack's image differs from the original where something else has redrawn the
   original's exact colour.

3. **The compositor** (`hd::compose`). `compose` runs `present`
   unchanged, then for every view pixel still owned by a blit whose name has an
   `HdImage`, fills that pixel's `sx` by `sy` block of window pixels from the HD
   image: the window pixel's place in the blit's rectangle, scaled to output pixels,
   picks the nearest texel (mirrored or flipped as the blit was). Texels with alpha
   below 0x80 leave the original pixel. So an HD image replaces exactly the pixels
   its blit put on screen and nothing else, it is clipped exactly as the original
   (the owners are only set inside the clip), and everything without an HD image is
   the original, byte for byte. With no recording or an empty map, `compose` is
   `present`.

   Because ownership is per logical pixel, an HD image cannot draw outside the
   original's silhouette, and inside it a transparent texel shows the original pixel,
   not what is behind it. That keeps the picture faithful and needs no depth or
   layer information; outlines stay at logical resolution, filled in at output
   resolution.

## Shapes at output resolution (T-236)

Bitmaps can only be replaced by other bitmaps, but the 3D view's hills and blocks, the
maps' hill octagons and the sloping lines are geometry, so they can be drawn again
at the window's resolution. Nothing else changes: the fill and line routines
still set every framebuffer pixel as the original does.

- **The record.** `Recording` keeps one list of `Record`s, blits and shapes
  (`ShapeRecord`: a `Geometry::Polygon` of corners or a `Geometry::Line` of two ends,
  in surface pixels, its colour, and the rectangle it was clipped to). The owner word is
  now a *stamp* and a colour: stamp `2k + 2` is record `k`, an odd stamp `2n + 1` a
  background drawn after `n` records (`clear`, `fill_rect`, the 3D view's ground and
  sky), and 0 a write the record did not see (text, sprites, pictures, dots). So the
  stamps order everything recorded. `shape::fill_polygon` records every polygon it
  fills; `mainview::draw_line` records its sloping lines (a level or upright line would
  look the same, so it is not recorded).
- **The copies.** The 3D view, the maps and the radars draw into their own buffers
  (the console and situation room `Framebuffer::new` them recording when the screen
  is), and `Framebuffer::copy_through` (slot `0x5a4`, which `review::copy_through`
  now calls) appends the buffer's records to the screen's, moved by the copy's offset,
  and gives each copied pixel its owner. A screen's record starts afresh
  (`restart_recording`) wherever `screens.rs` copies a whole picture, so it holds one
  frame's drawings. This is the part of T-234 the shapes need; blits ride along too.
The drawing at output resolution (`draw_shapes`, `--hd-shapes`) is T-236's second
part.

## What it costs

Measured on the development machine with a release build, drawing real scorpion
frames at 8 scales and 32 facings (`draw_model`), and presenting at 3840x2160:

| Path | Before T-070 | After |
|---|---|---|
| `draw_model`, not recording (the 1x path) | 32.8 us a model | 33.7 us (+2.6%) |
| `draw_model`, recording | | 51 us |
| `present`, 4K | 0.93 ms | 0.93 ms |
| `compose`, 4K, nothing replaced | | 0.93 ms (it is `present`) |
| `compose`, 4K, all 286 blits of a crowded frame replaced | | 9.6 ms |

The unrecorded blit runs the same loop as before (the row loop is generic over the
fill, so no recording branch is left in it). Recording and composing cost nothing
until a pack is loaded.

## For the pack (T-072) and UI text (T-074)

- T-072 loads PNGs into `HdImage`s (`0xAARRGGBB`, any size; the stretch maps the
  whole image onto the blit's rectangle, so a 4x pack and an 8x pack both work), keyed
  by `AssetId`, turns recording on for frames it shows, starts each frame's record
  afresh where the screen redraws, and calls `compose` instead of `present`. Missing
  names fall back to the original per asset with no extra code.
- Still to name at their call sites: sprites (`blit_sprite`), the screens and pictures
  copied in whole, skies and glyphs (T-233). The 3D view's copy into the console now
  carries the records (T-236, above); `window.rs`'s labels still draw over a clone.
- T-074's text can use the same records: a glyph blit is a rectangle and a name, and a
  sharp glyph is an `HdImage` drawn by `compose`, or rasterised at output size from
  the same rectangle.
