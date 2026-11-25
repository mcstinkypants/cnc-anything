# Burner

A browser app to design laser jobs and send them to a GRBL laser over USB.

## Start it

    python3 main.py

This serves the app on your computer only and opens it in your browser
(use Chrome or Edge: they are the browsers that can talk to USB devices).
Press Ctrl+C in the terminal to quit.

## What it does

- Draw lines, rectangles (with rounded corners), ellipses, polygons and text; import SVG drawings and pictures.
- Move, resize, rotate, mirror, align, copy/paste, undo/redo; type exact sizes in the bar at the top.
- Put shapes on colored layers, each with its own speed, power, passes and mode:
  Line (follow outlines), Fill (scan the inside, with overscan and cross-hatch) or Image (dithered or grayscale pictures).
- Preview every move with an estimated time, and save the G-code.
- Connect over USB (or to the built-in practice machine), then Home, Frame, Start, Pause and Stop;
  jog the head, set the origin and type commands in the console.

Projects save as `.burner` files (older `.lsproj` files still open); your current design is also kept in the browser between visits.
