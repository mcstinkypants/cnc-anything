# CNC Anything

A desktop app (Rust) to design jobs and send them to GRBL laser cutters and 2D CNC routers over USB.

## Run it

- **AppImage:** `dist/CNC_Anything-x86_64.AppImage`. Make it executable and double-click it, or run it from a terminal.
  If AppImageLauncher is installed it first asks whether to integrate the app; either answer works.
- **Debian/Ubuntu (x86_64):** `dist/cnc-anything_<version>_amd64.deb`, installed with your package manager.
- **From source:** `cargo run --release`
- **Build the packages:** `sh packaging/release.sh` (AppImage and .deb), or `build-appimage.sh` / `build-deb.sh` on their own.

## Versions

Every change is committed as `x.y.z - {feature|bugfix|chore|docs} - Description`, with the version in `Cargo.toml`
bumped to match. The title bar (and Help → About) shows the version and the git commit the program was built from;
a build made from uncommitted changes shows `-modified` after the commit id. Commit first, then build the packages.

On Linux, using a machine over USB needs permission for serial ports: your user must be in the
`dialout` group (ask your system's administrator, or see your distribution's help), then log out and back in.

## What it does

- Draw lines, rectangles (rounded corners), ellipses, polygons and text (real font outlines); import SVG drawings and pictures.
- **Spline tool** and **Edit points** tool: Bézier curves with sharp, corner, smooth and symmetric points; drag or type exact point and handle positions; add and delete points. Rectangles, ellipses, polygons, lines and text convert to editable curves.
- Move, resize, rotate, mirror, align, copy/paste, undo/redo; type exact sizes; snap to the grid and to object centers; dark canvas.
- **Machine profiles:** keep several named lasers and routers (bed, port, controller, router settings); create, edit and save them without a machine connected, switch from the toolbar, and export or import them as JSON.
- Switch the toolbar between **Laser** and **Router**.
- **Laser:** colored cut layers with speed, power, passes and Line / Fill / Image modes, and a material library.
- **Router (2D CNC):** each layer has an operation (Profile outside, Profile inside, On line, Pocket, Drill), a tool,
  a cut depth and depth per pass, feed, plunge and spindle speed, climb or conventional milling, and holding tabs.
  Profiles are offset by the tool radius so parts and holes keep their drawn size; pockets clear in rings at the chosen stepover;
  the job pauses for a tool change when layers use different tools.
- **Tool library** (number, type, diameter, flutes, flute length, V-bit angle) and **speeds & feeds library**
  (per material and tool: RPM, chip load, feed, plunge, depth per pass, stepover). Apply an entry to a layer in one click;
  each library exports and imports as JSON.
- Preview every move with a time estimate (router cuts are drawn at the tool's width, darker when deeper), save G-code or SVG.
- Connect over USB (or the built-in practice machine): Home, Frame, Start, Pause, Stop, jog X/Y/Z, set XY and Z zero, console.

Settings, libraries and an autosave of your design live in `~/.config/cnc-anything/` (settings from Burner are carried over).
Projects save as `.cnca` files; earlier `.burner` and `.lsproj` projects open too.

The starter tools and speeds & feeds are estimated starting points for a hobby router: test on scrap first.

`--screenshot FILE.png` opens the window, draws about 120 frames, saves a picture and exits.
