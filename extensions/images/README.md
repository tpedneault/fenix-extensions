# Image viewer

Pictures in a pane. A PNG, JPEG, GIF, WebP, BMP or SVG opens in it however
you open it -- the explorer, the file picker, `:e`, a recent file, the
command line, `gf` -- fitted to the pane, over the theme's background,
with a checkerboard under what's transparent and a details line below:
its name, format, size in pixels and on disk, how big it's shown, and its
place in its folder.

| Key | What it does |
| --- | --- |
| `+` `-` | Zoom in and out (no smaller than fitted) |
| `0` `=` | Fit the pane |
| `1` | Actual size: a point a pixel, as the system shows pictures |
| `h` `j` `k` `l`, the arrows | Pan |
| A drag, the wheel | Pan; zoom about the pointer |
| `]` `[` | The next and previous picture in its folder, round from the last to the first |
| `p`, `.` | Play or pause a moving picture; the next frame, paused |
| `s` | An SVG's source: its text, to edit |
| `d` | Hide or show the details line |
| `r` | Read it again (it changed on disk) |
| `q` | Close it |

These are the diagram viewer's keys (`SPC m v` in a Mermaid file). Zoomed
in three times or more, each pixel is drawn as a square, to see them;
shown smaller, it's drawn from a picture halved as often as it takes, so
fine detail doesn't shimmer. An SVG is drawn again at every zoom, so it's
sharp however close you look, and it fits the pane whatever its size;
its text is set in the system's fonts.

GIFs, animated WebPs and APNGs play, a frame at a time, at their own
pace (a frame asking for 10 ms or less is shown for 100 ms, as browsers
do), and stop while they're out of sight.

`SPC m` on a picture has the same, and `SPC f i` from anywhere previews
the picture the line you're on names -- `![plot](img/plot.png)`,
`src="logo.svg"`, `"sprites/cat.gif"`; the one under the cursor, when it
names more -- in a pane beside it, the focus staying in your code. Again
on the same picture closes it. A picture in Markdown's reading view opens
here too (`Enter` on it).

`SPC f o` goes from a picture to its file's text, and back.

| Key | Command | What it does |
| --- | --- | --- |
| `SPC f i` | `images.preview` | The picture this line names, beside it |
| `SPC m ]` `SPC m [` | `images.next` `images.previous` | The next and previous picture in its folder |
| `SPC m 0` `SPC m 1` | `images.fit` `images.actual_size` | Fit, actual size |
| `SPC m s` | `images.source` | An SVG's source, to edit |
| `SPC m p` | `images.play` | Play or pause |
| `SPC m d` | `images.details` | The details line |

The modeline says the same as the details line, for the picture looked at
last: `IMG cat.gif · GIF · 640 × 480 · 1.2 MB · playing, 12 frames · fit 50% · 3/12`.

## Limits

- **Big pictures.** An extension has 128 MiB, so a picture is decoded
  whole up to about 8.4 megapixels (a 4K screenshot). A bigger JPEG is
  decoded at a half, a quarter or an eighth of its size (the details line
  says so), which the pane doesn't miss until you zoom in past fitted; a
  bigger picture of another kind isn't shown, and says why.
- **Its folder.** `]` and `[` go through the folder a picture's in. One
  outside the project Fenix lists for it, and only the pictures in it.
- **The session.** A picture opens again with the session, at the zoom
  you left it: Fenix gives the extension again the files its pages show,
  outside the project too.
- **SVG source.** `s` opens the file's own text, to edit; `r` on the
  picture reads it again once it's saved. An SVGZ is compressed, so its
  `s` is a copy, read-only. `SPC f o` goes to the text of the file the
  pane was opened on, not the one `]` went to.
- **The preview** reads pictures in the project.

## What it may do

- Read the project you're in: a picture's folder, for `]` and `[`, and
  the pictures a line names, for the preview. Outside it, the pictures
  beside one you opened, which Fenix lists for it.
- Know where you are in the editor, for the preview: the file, the line,
  the cursor and its project.
- Read the system's fonts, for an SVG's text and the details line.

## Building

```text
cargo xtask pack extensions/images dist
```

`cargo test` checks the arithmetic of zooming and panning, the folder's
order, what a file is from its bytes, the pictures a line names, and the
pixels drawn: the checks, the theme, squares and halving, an SVG's edges.
