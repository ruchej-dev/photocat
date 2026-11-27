# assets

The face of the program: the mark (a cat head built from a stack of photo frames, an aperture for the
eye) and its lockup. ADR 0052 explains why the mark is an asset while every icon inside the window
stays drawn geometry, and why it is distributed under the same Apache-2.0 as the code.

| File | What it is | Who uses it |
|---|---|---|
| `photocat-1024.png` | the mark as the owner drew it: 1024×1024, the dark rounded plate with its glow | the source of the other files, the store and the press kit |
| `photocat-lockup.png` | the mark with the word `photocat`, 1184×296, dark background | the top of both READMEs |
| `photocat-icon-256.png` | the mark alone: flat amber `#F59E0B` on transparency, no plate, no glow | the window icon, embedded in the binary |
| `photocat-icon-128.png` | the same, 128 px | packaging: a `.desktop` entry, a macOS `.iconset` |
| `photocat-icon-64.png` | the same, 64 px | packaging |
| `photocat-icon-32.png` | the same geometry at 32 px, its alpha a little brighter (`α^0.78`), because a 20 px stroke of the 1024 px file is 0.6 px here | packaging: the smallest size the mark still reads in |
| `photocat-mark-24.png` | the same geometry at 24 px, `α^0.70` — the size of the mark in the top bar of the window | the top bar, embedded in the binary and **tinted** with the accent of the theme (`#f59e0b` dark, `#b45309` light), so one file is right in both themes |

## How the flat set was derived

The originals came as bitmaps (the mark was a JPEG under a `webp` name), so the flat set was cut out
of them once, by hand, and the files are committed as they came out — nothing regenerates them:

1. the bright amber strokes are separated from the plate by a rolling-ball subtraction (a 31 px
   opening plus an 18 px blur of the mask estimates the baked glow, and the glow is subtracted from
   the mask), leaving `alpha = f(max channel)`;
2. the plate and its glowing rim are cut off 60 px from every edge, and islands under 400 px are
   dropped as specks;
3. the mark is cropped to its dense part with a 5 % margin, and painted in one flat colour with that
   alpha;
4. `256 / 128 / 64` are Lanczos reductions of the 680 px mask; `32` is the same mask with a gamma on
   the alpha, the one parameter that was tuned by eye on a `#0C0E12` and a `#F8FAFC` background.

## What the 32 px file does not do

At 16 px the strokes drown, whatever the gamma: a favicon-grade square needs a filled silhouette
(the head filled, the aperture cut out), which is a drawing and not a reduction. It is not made yet —
nothing in the program asks for 16 px, and the task that needs it (a web page or a Windows `.ico`)
says so in `plan/BACKLOG.md`.

## Rules

- A new size is a file, not a script at build time: the binary embeds exactly one PNG, and a missing
  asset costs the icon, not the window.
- The mark never appears *inside* the window as a widget or a background (ADR 0048 stays true: the
  interface is drawn geometry, and its screenshots do not depend on a bitmap).
- `photocat-1024.png` and `photocat-lockup.png` are the owner's originals: keep them as they came.
