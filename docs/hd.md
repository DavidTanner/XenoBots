# Resolution-independent sprites (T-070)

How higher-resolution art can replace the original's images one at a time, at output
resolution, without changing what the game draws. This is the groundwork for the HD
asset pack (T-072, "The HD pack" below) and sharp UI text (T-074). No HD art exists in
the repository; a pack is always the user's, loaded at run time.

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
   - `Sprite { bank, index }`: record `index` of `msprite{bank}.b@n` (from 0, as
     `--view` pages them). The game draws a sprite by its directory handle, and the
     banks sit at fixed places in that one directory (`docs/formats/b@n.md`, "Sprite
     numbers"), so `AssetId::sprite(handle)` names it: every `blit_sprite` in
     `xeno-app` sets `SpriteBlit::asset` from the handle it looked the sprite up by
     (T-233), and the viewer from the bank's file name and the page.
   - `ModelImage { model, view, image }`: image `image` of view `view` of a model,
     views numbered along the loaded chain (`.m08` first, then `.m16`, `.m32`, as
     `LoadedModel` keeps them), or of its `.m00` when `view` is `None`. `model` is the
     file's base name (`hd::Name`, 8 bytes, lower case; `LoadedModel::name`, set by
     the app's loaders). A frame is a list of these images placed at offsets
     (`docs/formats/mxx.md`), and frames share images, so a pack replaces images, not
     frames: one HD image fixes every frame and facing that uses it.
   - `File(name)`: a whole file drawn as one picture: a `.m@l` screen, a `.vcl`
     picture, a sky strip (`AssetId::file("maproom.m@l")`, the extension dropped).
     Pictures are copied in, not blitted, so `Framebuffer::copy_picture` copies one,
     starts the record afresh and records it, and `record_picture` records any
     rectangle as one 1:1 blit owning the pixels in it as they are (T-233):
     - **Screens** (`screens.rs`) and the viewer's pictures cover their rectangle.
     - **`.vcl` pictures** (`intro.rs`, `Op::Load`) are recorded over the whole
       640-wide surface. The record is in surface pixels and `compose` picks the
       texel by surface position, so as the cutscene pans the 320x240 view (the
       origin) the HD picture pans with it, with no record per frame.
     - **Skies** (`view3d::draw_sky`, named by the scenario's `BGND` through
       `Mission::sky`) are recorded as one whole 342x54 strip per wrap: the strip's
       left edge at `-first`, `342 - first`, ... (`first` the strip column at x = 0)
       and its top at `rows - 54`, so its last `rows` rows end at the horizon. Each
       sky pixel is owned by the wrap it came from, so an HD strip is cut, scrolled
       and wrapped exactly as the original; the rows above the shown ones simply
       fall off the buffer. The records ride through the 3D view's copy into the
       console like the models'.

     A known gap: HD texels are colours, not palette indices, so they do not follow
     the palette's fades (screens fading in, the cutscenes' fades) or colour
     animation (T-241).

   Glyphs could add a `Glyph { font, code }` name the same way (T-239).

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
maps' hill octagons and the sloping lines are geometry, so `--hd-shapes` draws them
again at the window's resolution. Nothing else changes: the fill and line routines
still set every framebuffer pixel as the original does.

- **The record.** `Recording` keeps one list of `Record`s, blits and shapes
  (`ShapeRecord`: a `Geometry::Polygon` of corners or a `Geometry::Line` of two ends,
  in surface pixels, its colour, and the rectangle it was clipped to). The owner word is
  now a *stamp* and a colour: stamp `2k + 2` is record `k`, an odd stamp `2n + 1` a
  background drawn after `n` records (`clear`, `fill_rect`, the 3D view's ground and
  sky), and 0 a write the record did not see (sprites, pictures, dots; text before T-074). So the
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
- **The outline.** In surface pixels with a pixel's centre at its whole number, a
  polygon covers its convex outline grown by half a pixel each way (the Minkowski sum
  with the pixel square: each edge moved out by `(|nx| + |ny|) / 2` along its normal
  `n`, and the corners' bounding box grown by a half), which matches the fill's
  inclusive left, right, top and bottom; a line covers the band one pixel thick across
  its longer axis, half a pixel past each end, as a line stepping one pixel along that
  axis does. Non-convex and zero-area polygons are left as the original's pixels (a
  `shortcut:` comment; the shape ops' triangles and quads are convex).
- **The edge rule.** `draw_shapes` runs after `present` (or `compose`), shapes in
  record order. For each shape it takes its *edge* pixels (owned, with a neighbour it
  does not own) and the neighbours of those that it may cover: in its clip, and drawn
  *before* it (a background or an earlier record; never a later record or an unseen
  write, so text, sprites, models in front and the pointer stay on top). In those
  pixels' output blocks, an output pixel whose centre is in the outline takes the
  shape's palette colour; one in the shape's own pixel that is not takes the colour of
  the neighbour on that side (across first if it is farther across than down, then
  down, then diagonally) if that neighbour was drawn before the shape, else keeps the
  shape's. Pixels whose eight neighbours are all the shape's keep `present`'s colour.
  So a shape moves by less than one logical pixel, only into what was behind it, and in
  the original palette's exact colours: no blending, no anti-aliasing.
- **Not covered.** The map's grid and objects, the radars and every marker are single
  dots or sprites, the same at any resolution; axis-aligned rules and frames likewise.
  A split view (the intro drop) is left as presented.

With the option off, nothing records and `window.rs` calls `present` alone, so the
output is `present`'s byte for byte. On the development machine (release, 3840x2160,
8x10), `present` takes 1.0 ms and `draw_shapes` on the stock missions' first frames
up to 0.8 ms for a situation room map and 1.1 ms for a console with hills in view
(scen06a); nothing to do costs under a microsecond.

## Text at output resolution (T-074)

`--hd-ui` does for text what `--hd-shapes` does for shapes. Every glyph still goes
into the framebuffer through `Framebuffer::draw_char`, pixel for pixel as before, and the
glyph shapes come only from the user's `mgavideo.dvr` at run time: nothing about the
fonts is in the repository.

- **The record.** When recording, `draw_char` pushes a `Record::Glyph` (`GlyphRecord`):
  the code, the cursor (the cell's top-left corner), the cells it drew (`width` by
  `height`, each 0 or the value 1 to 3 drawn as `colour + (value ^ 3)`; a bitmap
  font's set bits are 3), the colour, the background if opaque, the clip (the
  surface, moved along by `copy_through` like a shape's) and the colours the surface
  had under the cell and one pixel round it (`under`). Its pixels get its stamp, so
  text is ordered with the shapes and blits around it. A glyph with no drawn cell (a
  space) is not recorded. The record copies the cells instead of naming the font, so
  drawing needs no font table; naming glyphs for a pack is T-239.
- **What it may draw on.** Only the glyph's cell changes, and in it only a pixel the
  glyph still owns or one that still shows what the glyph was drawn over: the colour
  in `under`, and no later record's stamp. That takes text over pictures copied in
  whole and over pixels the record never saw (`recolour`, `set`), which have stamp 0,
  while a later write in another colour (the pointer, a sprite, a later glyph) stays
  on top. The same-colour gap of the owners applies.
- **The corner rule.** Each cell's four corners are looked at, beyond the line through
  its edges' midpoints (`|fx| + |fy| > 1/2` from the cell's centre). With `P` the cell,
  `H` and `V` the cells beside and above or below that corner and `D` the one
  diagonally across: if `H` and `V` hold the same higher value and `D` does not, the
  two touch diagonally across `P`'s corner and it is filled in their colour; if `H`,
  `V` and `D` hold the same lower value and `P` is one end of a diagonal step (the
  cell diagonally across its other side holds `P`'s value and the cell between does
  not), the corner is cut to the colour of the pixel beside it. So a 45-degree stroke
  gets straight edges, while level and upright strokes, their ends and the insides of
  corners keep the original's square pixels. The shaded fonts go through the same
  rule on their values, so each shade band keeps its own palette colour: no blending.
  Steps other than 45 degrees are left as they are.
- **Approaches tried** (4K PNGs of the orders view, the log, the status page, the map,
  a console and the intro's first caption): this rule ("diagonals"), and marching
  squares ("corners": every convex corner cut and every concave one filled, saddles
  joined). Corners also smooths the diagonals, but it turns every stroke end into a
  point and chamfers every corner, so font 2 (the log's, 4 by 6 and nearly all level
  and upright strokes) and the captions' `I`, `T` and `E` no longer look like the
  original's type. Diagonals changes only where the original has a stair step, so it
  keeps the typeface and is the one kept. Bilinear interpolation thresholded at a half
  was ruled out on paper: it never joins two cells that touch only at a corner.
- **The window's labels.** The speed label and the key legend are drawn with
  `draw_text` over the frame's clone, so with recording on they are recorded and drawn
  like the game's text; `--hd-ui` covers them with no extra code.
- **Turning it on.** `window.rs` records when `--hd-shapes` or `--hd-ui` is on and calls
  `draw_glyphs` after `present` and `draw_shapes`. A screen that redraws text without
  copying its picture again grows the record, so past 65,536 records it starts afresh
  before the frame's drawing (a `shortcut:` comment): text drawn before then is not
  smoothed until it is drawn again. A split view (the intro drop's credits) is mapped
  both ways, above and below the split.

With the option off, nothing records and the output is `present`'s byte for byte. On
the development machine (release, 3840x2160, 8x10) `draw_glyphs` takes about 0.2 ms
on the situation room's orders view and log, 0.17 ms on the status page and 0.07 ms on
a console, against the 14.3 ms frame budget.

## The HD pack (T-072)

`--hd-pack <dir>` loads the user's replacement images at start-up and draws them over
the blits they name. Nothing of a pack is ever in the repository.

- **Names.** One PNG per image, at a path under the directory made from its `AssetId`
  (`AssetId::pack_path`, `from_pack_path`), lower case (any case is read):

  | `AssetId` | Path | Example |
  |---|---|---|
  | `ModelImage { model, view: Some(v), image }` | `model/<model>/<v>_<image>.png` | `model/scorpion/3_12.png` |
  | `ModelImage { model, view: None, image }` (the `.m00`'s) | `model/<model>/m00_<image>.png` | `model/netrelay/m00_0.png` |
  | `Sprite { bank, index }` | `sprite/<bank>/<index>.png` | `sprite/1/42.png` |
  | `File(name)` | `file/<name>.png` | `file/sitroom.png` |

  Numbers are decimal with no leading zeros, so each image has one name; views count
  along the loaded chain (`.m08`'s first, then `.m16`'s, `.m32`'s). T-239 adds
  `glyph/<font>/<code>.png` with `AssetId::Glyph`.
- **Size.** Each side a whole multiple of the original image's (4x4, 8x8 and 8x10 all
  work). The whole image is stretched evenly (nearest texel) over the rectangle the
  blit covers, however the original scaled it, so the multiple only sets the detail.
- **Transparency.** PNG alpha: a texel with alpha below 0x80 shows the original's
  pixel there, at or above it the texel's colour, opaque (no blending). Colour 0 is
  transparent in the original's blit, so those pixels are never the blit's and the
  pack cannot draw there either: an image only repaints its original's silhouette.
- **Checks.** Only images the user's game files have can be replaced: the loader takes
  each original's size (`xeno_render::hd::originals`: `LoadedModel::images` for every
  model, every sprite of `msprite0` to `msprite5`, and every `.m@l`, `.vcl` and
  `sky*.b@n` in the game directory) and skips,
  with a warning in the log, a file whose path names nothing, an image the game does
  not have (`model/scorpion/0_99.png`), a PNG that does not decode and one whose size
  is not a whole multiple. Those images stay the original's. A directory that cannot
  be read is an error before the window opens.
- **Drawing.** With a pack, `window.rs` records every frame (as for `--hd-shapes`) and
  calls `compose` in place of `present`, then `draw_shapes` and `draw_glyphs` if those
  are on, so smoothed shapes and text stay over the pack's images as they are over the
  original's. With no pack and neither flag, the frame is `present`'s byte for byte.
- **Checked.** A synthetic pack (`an_hd_pack_replaces_one_model_image_and_leaves_the_rest`)
  and one solid-colour image over the first model image seen in a training mission's
  console (`real_assets_an_hd_pack_replaces_only_its_model_image`: the netrelay's `.m00`
  image in NTRAIN), and likewise one sprite (`real_assets_an_hd_pack_replaces_only_its_sprite`:
  `sprite/3/65.png`, a piece of the Scorpion console, T-233), the console's screen
  and the 3D view's sky (`real_assets_an_hd_pack_replaces_only_its_screen_and_sky`:
  `file/scorpcon.png`, `file/sky_a.png`): only that image's owned pixels change. A
  synthetic `.vcl`-like picture on a wider surface (`an_hd_picture_pans_with_the_view_over_a_wider_surface`)
  checks the pan. The console's 3D view
  reaches the screen's record through `copy_through` (T-236).
- **Starting a pack (T-240).** `xeno-tool dump <game dir> --hd-template <dir>` writes
  every image `originals` lists (1,201 in a stock install: 781 model images, 390
  sprites, 8 screens, 14 pictures, 8 skies; 6.4 MB, 0.6 s in release) at its path
  under `<dir>`, each pixel a 4x4 block (`hd::Original::upscaled`), so an artist sees
  which view and image number a picture is and paints over it. Colours: a screen and a
  picture its own palette; images with none the palette of the screen the game shows
  them on (as `--view` picks, T-035): a bank the screen it is loaded with (`msprite0`,
  `msprite1`: the situation room; 2, 3, 4: their consoles; 5: the map room), a model
  its own robot kind's console (the Humanoid's for the rest), a sky the Humanoid's
  console (the consoles differ only in colours 253 and 254, `mxx.md`, "Palette").
  Colour 0 is alpha 0 in model images and sprites (where it is not drawn, and where an
  opaque sprite draws it the original's pixel shows, the same colour); whole files are
  opaque. A PNG already in `<dir>` is kept, so running it again never overwrites an
  artist's work, and a `<dir>` inside this repository is refused (the template is the
  game's art). `real_assets_the_hd_template_composes_as_the_original_presents` writes
  it, loads it with no file skipped and composes every stock mission's situation room
  and first console at 4K: 25 of 34 frames are `present`'s exactly; in the other nine
  only far models differ, model images drawn below 1:1 (scales 2 to 23 of 256), which
  the original draws from some of their pixels (the repeat table, `mxx.md`,
  "Drawing") while a pack image is stretched evenly over the blit's rectangle. The
  palette's tint and colour animation are not applied to pack images (T-241).
- **Cost** (release, 3840x2160, the most model pixels of any stock robot's first
  console frame, SCEN06A, 401 blits): `present` 1.0 ms, `compose` with one image
  replaced 1.39 ms, with all 781 model images replaced 1.41 ms; loading 781 4x PNGs
  68 ms. With the sprites named too (T-233; every stock mission's situation room and
  first console, all 1,171 model images and sprites replaced in memory): `present`
  0.9 ms, `compose` 1.65 to 1.69 ms on a situation room and 2.2 to 2.8 ms on a console
  (the Scout's, SCEN08A, the slowest), against the 14.3 ms frame. With the screen
  and sky replaced as well (1,201 images), every window pixel of the frame is an HD
  texel and `compose` took 9.9 ms; working each block's texel columns out once and its
  rows once per view row (rather than two divisions per window pixel) brought that
  to 6.0 to 6.2 ms. What is left is mostly writing the window block by block rather
  than row by row (T-243). A frame records at
  most 599 drawings (SCEN07A's console; 46 to 78 of them sprites), and each full
  redraw of a mission view or the map room starts the record afresh, so the 65,536
  cap in `window.rs` is never near.

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

- T-072 loads the pack (above, "The HD pack").
- Still to name: glyphs (T-239) and the intro drop's `.pcx` backdrops (T-242).
  Sprites, screens, pictures and skies are named (T-233). The 3D view's copy into the console now
  carries the records (T-236, above); `window.rs`'s labels still draw over a clone.
- T-074's text uses the same records (above, "Text at output resolution").
