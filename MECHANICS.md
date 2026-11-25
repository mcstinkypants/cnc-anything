# Mechanics sheet

Values and rules read from the original program's main executable (`usr/bin/LightBurn`
inside the AppImage, sha256 23a4b993…). Addresses are the built-in engine's addresses
(Ghidra shows them +0x100000). Nothing here is copied code; each rule is re-implemented
in `src/gcode.rs` and `src/machine.rs` (Rust; the earlier browser version in `web-version/` used the same rules).

## Job header (function 0x416d20)
- Writes `G00 G17 G40 ` then `G21 ` (millimeters; `G20 ` for inches) then `G54`, newline.
- Coordinate decimals: 3 in millimeters, 4 in inches (field +0x174 set to 3 or 4).
- Start-from mode 2 (absolute) writes `G90`; mode 0 (current position) writes `G91`
  and every move is relative; mode 1 writes `G90` plus an offset move.
- Optional `; GCode clustering enabled (max …)` + `G4 P1` (not used here).
- Header comments `; Bounds: X… Y… to X… Y…` (string 0x139343d).

## Device types (strings near 0x13b2ba8 / class names)
- "GRBL 1.1f or higher, using variable power mode (M4)" → our `grbl` profile writes `M4`.
- "GRBL 1.1e or earlier, using constant power mode (M3)" → our `grbl-m3` profile writes `M3`.
- "S-value max … corresponding to 100% power. This must match the $30 setting in GRBL."

## Layer comment (function 0x426c90, around 0x426f5e–0x42706e)
`; <layer name> @ <speed × unit factor> <unit text>, <power with 1 decimal>% power`
(strings `; ` 0x136eb9b, ` @ ` 0x13794bd, `, ` 0x1badcd1, `% power\n` 0x139274e).
We print speed as mm/min.

## Moves (function 0x419640)
- Modal motion word: `G0` (0x13920a0) or `G1` (0x13ffdfe) is written only when the motion
  type changes (state at +0x104).
- Axis letters follow directly with no spaces (`G1X10Y5`).
- An axis word is left out when it moved less than **0.01 mm** (float at 0x13742bc).
- Relative mode accumulates the rounded output so rounding never drifts (adds to +0x180/+0x188).
- Feed `F` is written only when it changed by more than 0.01 (+0x198 holds the last one).

## Feed rate (function 0x4176c0)
- F = speed (mm/s) × **60** (float 0x137b984), printed with (coordinate decimals − 1) = 2 decimals.
- In inch mode the value is divided by **25.4** (float 0x13742a0).

## Power (function 0x4182e0, around 0x4185e5)
- S = power % ÷ **100** (float 0x13742c0) × S-value max (field +0x90), printed with 1 decimal,
  written only when it changes (+0x19c holds the last value).

## Number format (function 0x4166f0)
- Rounds to N decimals (powf(10, N), roundf), drops trailing zeros; a device option drops the
  leading zero (`0.5` → `.5`, `-0.5` → `-.5`).

## End of job (function 0x41bbe0)
- Absolute modes: `G90`, then optionally `; return to user-defined finish pos` + `G0 X… Y…`.
- Current-position mode: `; return to starting pos` + `G0 X<-dx> Y<-dy>` back to where it began.
- Laser off before: `M05`-family text from the device (0x139207c `M05\n`); we write `G1 S0`, `M5`.

## Jogging, origin, fire (strings 0x1392410–0x139245b, 0x13920a3)
- Jog command family `$J=` with `G91` and `G21` → `$J=G91 G21 X… Y… F…`.
- Set origin here: `G10 L20 P1 X0 Y0`.
- Fire / test pulse sequence uses `M3`, `G1 F100 S…`, `G4 P…`, `G1 S0`, `M5` (function 0x417220).
- Air assist on/off text "Air On"/"Air Off" (0x137965e) → `M8` / `M9`.

## Layer settings (setting names near 0x13cab00)
Modes include Line, Fill, Offset Fill, Image (Dithered, Hacktone, 3DSlice); per-layer keys include
`bidir`, `crossHatch`, `overscanPercent` ("Distance to overscan, as a percentage of speed"),
`scanOpt`, `priority`, `numPasses`, `minPower`, `maxPower`, `speed`, dither modes
`jarvis`, `stucki`, `atkinson`, `ordered`, `newsprint`, `halftone`, `grayscale`, `sketch`.
We implement Line, Fill (with bidirectional + cross-hatch + overscan) and Image
(Jarvis, Stucki, Floyd-Steinberg, Atkinson, ordered, threshold, grayscale).

## Controller messages (function 0x40eaf0)
- `error:` and `ALARM:` lines are explained with plain-language messages; e.g. jog beyond travel
  (error:15), "Abort during cycle" alarm after a reset while moving.

## Estimated (looked for, not found yet)
- Default S-value max: **1000** (GRBL's own $30 default) — estimated.
- Serial streaming: character-counting with a **127-byte** window, status `?` every **250 ms** — estimated.
- Default layer values (speed 100 mm/s, power 20 %, interval 0.1 mm, overscan 2.5 %) — estimated.
- Default bed 400 × 400 mm — estimated.
- Fill row spacing starts half an interval in from the edge — estimated.
- Final `M2` and the `G1 S0` before `M5` — estimated.

## Our additions (not from the original)
- Origin corner: (0,0) at the upper-left by default (Device settings can switch to lower-left).
  Rulers, the X/Y boxes, jogging and G-code all count Y down from the top edge in that case.
- Snapping: grid snap (follows the visible grid or a fixed 0.1-10 mm step) and snap to the centers of
  other objects and the bed, with guide lines while moving. Hold Alt to turn snapping off for a moment.

## Material library (our addition)
- A material stores a name, thickness, operation (Cut, Engrave, ...), notes and a cut profile:
  mode, speed, power, min power, passes, interval, both-directions, cross-hatch, overscan, air assist,
  picture style and invert. Applying it copies the profile onto a layer and labels the layer with it.
- Kept in `~/.config/burner/materials.json`; export/import uses the same JSON
  (`{"app":"cnc-anything","kind":"material-library","version":1,"materials":[...]}`; a bare list also imports).
  Importing replaces a material with the same name, thickness and operation and adds the rest.
- The starter materials are starting points for a 5-10 W diode laser: estimated, not from the original.

## 2D CNC routing (our addition; the original program is laser-only, so none of this comes from it)
Implemented in `src/cam.rs`, `src/tools.rs`; all values below are our own choices (estimated).
- Z0 is the top of the stock; cuts go to negative Z. Moves between cuts happen at the safe height (default 5 mm).
- Depth passes: n = ceil(depth / depth-per-pass) equal passes of depth/n (6 mm at 2.5 mm per pass → 2, 4, 6 mm).
- Profile outside / inside: the closed outlines form a region (outlines inside outlines are holes) that is grown
  or shrunk by the tool radius (polyline offsetting with arcs, flattened to 0.2 mm chords), so parts and holes keep
  their drawn size. Shapes too small for the tool are skipped with a warning.
- Climb milling (CW spindle): outside cuts run counter-clockwise around parts, inside cuts and pockets clockwise;
  conventional reverses both.
- Pocket: the region shrunk by the tool radius, then again by stepover % × diameter until nothing is left;
  innermost ring first, the wall pass last, all rings at one depth before the next. Rings link 0.5 mm above the stock.
- On line: the tool center follows the drawing. Open paths in profile layers are cut on the line.
- Drill: center of each closed shape, pecking one depth-per-pass at a time and lifting to 0.5 mm between pecks.
- Holding tabs (profiles): count per shape, evenly spaced along the path, width + tool diameter long; passes deeper
  than (depth − tab height) rise to the tab top over them.
- Order: holes and inner shapes first, then nearest next; each path to full depth before the next (pockets excepted).
- G-code: `G21 G90 G17 G94`, `G0 Z<safe>`, `M3 S<rpm>` + `G4 P<spindle wait>` (2 s) when the speed changes,
  plunges at the plunge rate, cuts at the feed rate (mm/min), `G0 Z<safe>`, `M5`, finish position, `M2`.
  A different tool on a later layer gives `G0 Z<safe>`, `M5`, a `; TOOL CHANGE:` note and `M0` (pause until Resume).
- Feed from a speeds & feeds entry = RPM × flutes × chip load. Starter chip loads (mm/tooth per mm of tool diameter):
  softwood 0.016, hardwood 0.012, plywood 0.013, MDF 0.018, acrylic 0.012 (single flute), aluminum 6061 0.0025
  (single flute); 18,000 RPM (acrylic 16,000); depth per pass 0.5 × diameter for wood; plunge = feed / 3.
- Libraries: `~/.config/cnc-anything/tools.json` and `feeds.json`; import replaces the same tool number, or the same
  material + tool (a tool is found again by name when its number differs).

## Resizing (our addition)
- Stretching folds the scale into the shape's own size (QR split of the matrix: rotation/mirror and slant stay,
  the scale goes into width/height), so rounded-rectangle corner radii, ellipses and polygons keep their true shape.
- Text: the height scale becomes the new text size (outlines scaled with it); a width that differs from the
  height is kept as a stretch, shown as an orange "↔ N% width" tag on the canvas and next to the Size box,
  with an Unstretch button to return to the font's normal proportions.
- X/Y refer to the chosen anchor point of the selection (any corner, side center or the center); typed
  W/H changes keep that point in place.

## Curves, splines and point editing (our addition)
- Curve shapes ("spline") are cubic Bézier curves: each point has an incoming and outgoing handle stored as positions.
  Point types: Sharp (no handles), Corner (handles move on their own), Smooth (handles stay in line, own lengths),
  Symmetric (in line and equal length). Curves are drawn with 0.25 mm chords for cutting and preview.
- Spline tool: click = sharp point, drag = symmetric handles (Shift+drag = corner), click the first point to close,
  Enter / double-click / right-click to finish.
- Edit points tool: drag points (with grid and center snapping) and handles; double-click a segment adds a point without
  changing the curve (de Casteljau split); double-click a point toggles sharp/smooth; Delete removes a point.
  The toolbar shows the point's and both handles' X/Y (in the chosen origin's coordinates) for typing exact values.
- Convert to curve: rectangles (rounded corners as exact Bézier arcs, handle = 0.5523 × radius), ellipses (4 symmetric
  points), polygons and lines (sharp points), text (the font's own quadratic/cubic outlines, so letters keep real curves).

## Machine profiles (our addition)
- Several named machines in `~/.config/cnc-anything/machines.json` (`{"kind":"machine-profiles","active":<name>,"machines":[...]}`),
  each with machine type, bed size, serial port, baud rate, S-value max, controller, router settings and origin corner.
- Created, edited, saved, duplicated, deleted, imported and exported in the Machine profiles window without a connection;
  the port can be typed even when the machine isn't plugged in. Switching machines (toolbar picker) changes the bed and
  job settings at once; a connected machine keeps its connection until you reconnect.
- The first time, the machine set up earlier becomes the first profile and an example of the other kind is added.
