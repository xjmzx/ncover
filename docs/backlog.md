# Backlog

Planned work that is agreed in outline but not started. Each entry says why,
what, and what is still open. Remove an entry when it ships (the CHANGELOG
takes over from there).

## The cover review loop (planned 2026-11-13)

The items below fit together into one loop for keeping a library's covers in
shape. Each app answers only what it alone knows. The apps share plain files,
never a database.

1. **ndisc**: Local covers vs Blossom, then Export list.
2. **ncover**: From a list…, filtered to the problem at hand (non-square,
   oversized, undersized). Then Square or Cap, or replace a file by hand
   via Open folder.
3. **ncover**: write the results into the library, keeping the originals.
4. **ndisc**: scan library, check again, upload, publish.

Built: all four steps. ndisc's export (0.7.6), and in ncover the list
scope, Square, Pad, the review grid (0.4.0) and the library write-back
(0.6.0), and Convert (0.7.0). Next: the dimension filters.

## Label art: the same loop, for label images (noted 2026-11-20)

**Where it stands.** ndisc 0.8.0 (released 2026-11-20) keeps labels in its
database, has a label art folder setting (the masters folder, matched to
labels by name, with a hand pick per label) and checks each label's local
file beside its published image in its Review overlay. It does **not yet
export label lines**: that is ndisc's stage 3c, and until it lands there is
nothing for ncover to read.

**Added the same day (by the user): a Label art scope** — a fourth Scope
button over `<library root>/_labels`, for running a recipe over every label
image at once. Lists stay the way to act on the flagged few.

**Decided 2026-11-20 (by the user): labels stay list lines.** No Labels
section and no Covers / Labels filter in Batch. ndisc decides which labels
need work (it knows what is published) and exports them as their own list;
ncover acts on plain files. Covers and labels never meet in one run because
they are two exports. Considered and set aside: a Covers / Labels filter
(nothing to separate while the exports are separate) and a Labels section
pointed at the label art folder (Batch's folder scope already does that,
without knowing what needs work).

Optional polish, not agreed, if the list lines ever feel anonymous: name
a one-folder list in the readout ("Label list · tjmzx-labels"); suggest
Pad over Square for logos (a centre crop can cut a logo's type); say "for 5
labels" rather than "in 1 folder".

**ncover's side is built, not released.** Label art reaches ncover only as
list lines: one line per label, all pointing
into one folder of logos. `3ccf23d` lets a list name several files in one
folder (it took only the last before). With the library write-back
(`cd4a8a8`) and the Mac-parity Batch changes (`6a7fb21`), that is everything
since v0.4.0, all unreleased.

**ndisc 3c is built** (2026-11-20, after 0.8.0): Review → Labels → Local
files → Export list writes one line per flagged label file, in the cover
list's format. The real export (76 lines) loads here with nothing skipped;
Square takes 5 logos and Cap 14 once named files ignore the name filter
(fixed the same day; the default "cover" filter dropped every label file).

**Still to do, in order:**

1. ~~ndisc 3c~~ (built; needs an ndisc release to reach the installed app).
2. **Release ncover 0.5.0** with the write-back and the list fix, so the
   installed app can take that export.
3. **First real run**: ndisc's label export → ncover From a list… →
   Square / Pad (Pad was built partly for logos) → Into the library with
   an originals folder → ndisc rechecks. Watch: the originals land under
   `originals/<labels folder>/<file>`, and ndisc's match still finds each
   file (names are kept, so it should).
4. **ndisc 3d**: upload a label's local file and point the label at it,
   then publish the labels manifest.
5. **macOS**: Phase F7 (several files in one folder) before that app takes
   a label list; F1 for the write-back.

## Next build: notes from the first real Convert (2026-11-23)

From the user, after converting `_labels` into the library (6 written, 2
refused for transparency; log and originals checked). Fixes first, then
features.

**Fixes (0.7.1)**

- **The library confirm counts every PNG.** "Replace 8 covers" when 6 will
  be written: the number is read off the Run label, which counts PNGs, and
  the 2 transparent ones are refused at run time. Count what the run will
  write, and name the refusals ("6 covers · 2 PNGs with transparency will
  be refused"). Under the Label art scope say "labels", not "covers". The
  "rescan in ndisc" sentence was written for covers; check whether labels
  need it.
- **Convert's recipe readout is stale.** Under Convert the line still reads
  "→ 600×600 PNG" and the size dropdown is still shown, though neither
  applies: hide the size, and read "→ JPEG, same size".
- **Cover and Fit offer to run every file**, "Run (320 files)", when 276 of
  the 319 label images are already 600 × 600 and the recipe says 600 px.
  They are framings, not tidying jobs, so they write every file by design;
  but it reads as "320 need work". Either say so in the survey line ("every
  file is rewritten as a square PNG, including the 276 already 600 px"),
  or let them skip a file already square at the canvas size.
- **Remember the last list's folder.** `List…` opens at a default each
  time; open it where the last cover list was loaded from (ndisc's exports
  live in `~/Documents/N-suite/ndisc`).

**The "Job" question.** The dropdown labelled with its current value
("Cover") holds two kinds of thing: framings (Cover, Fit) that write a new
square PNG for every file, and jobs (Cap, Square, Pad, Convert) that tidy
only the files that need it. Convert was missed at first because nothing
says it is there. Options: rename the row ("Job"), split it into two, or
group the entries with a separator. Not decided.

**Why nothing finds the small labels (checked 2026-11-23).** Every job
filters only one way. Cap finds images *over* the size and never enlarges;
Square and Pad find images that are *not square*. Over `_labels` today all
319 readable images are square and none is over 600, so all four find
nothing — while 43 are under 600, 9 of them under 400 (Censor Records and
UIQ at 120, Werk Discs at 150, Play It Again Sam, Science and ~scape at
200, Believers Roast at 250, Accidental at 300, Tabernacle at 390; 10-30
Uproar and ECM are 400). That is exactly the planned **long side under N
px** filter below, and it is the next feature. Note what it is for:
finding them, not fixing them. Enlarging invents no detail, so the remedy
for a 120 px logo is a better source (ndisc already flags these as
`low-res<400` and gives them the `resource` action). The filter should
make that list easy to see and hand back, maybe as its own Export.

**Features, later**

- **A square border / frame** at 20, 40, 80 or 100 px, drawn round the
  image (Edit, and perhaps a Batch job). Open: inside the canvas (the art
  shrinks to make room) or outside it (the canvas grows); the colour from
  the picker, like the background.
- **Desaturate / greyscale**, if feasible: an `Operation` in the
  composition (so it has its vector form via an SVG colour matrix, as
  Invert does), and perhaps a Batch job. Fits the monochrome-by-default
  rule for new sites.

## Dimension filters and the Files list

**Built 2026-11-19:** the `From a list…` scope, reading ndisc's Export list
(folder · cover file · artist — title · disk WxH · published WxH · format ·
kinds · actions, tab-separated, `-` = empty). The framing picks the lines
(`ncover:cap`, `ncover:square`), only the named cover file is taken, and the
survey counts the lines left out. Still to build:

- ~~**Convert (PNG → JPEG)**~~ — **built 2026-11-22**, after the Mac built
  it first (1.1.0). The framing dropdown's sixth entry, taking the
  `ncover:convert` lines; quality 92, same pixels, non-PNGs left alone, a
  transparent PNG refused. Into the library too, which the Mac still
  refuses: the log's 11th column (Convert lines only) records the removed
  PNG, and Put back uses it. The Mac follows in the same wave. Order when a
  cover needs several jobs: square, then cap; convert joins whichever run
  writes last.
- **Filters** beside the name filter, for a folder or the manifest scope:
  non-square by more than X%, long side over N px, long side under N px.
  They join the walk's selection, not a separate pass, so the survey line,
  the Files list, the samples and the Run count all agree with them.

## Square: what is left (planned 2026-11-13)

**Why.** Every reader shows a cover in a square. nview, nplay and the glmps
grids and lists fill the square and crop the long side evenly at both ends
(`object-cover`). The glmps release page shows the whole image inside a
square frame with bars (`object-contain`). So a non-square cover is never
broken, but its edges are trimmed in most places. That only matters where art
or type runs to the edge, or where a scan includes a CD case hinge strip or a
scanner border. It is a per-cover judgement, not a library-wide rule.

At the time of writing, of ~2,000 covers in the published discography, 476
are within 2% of square, 127 are 2–10% off and 125 are more than 10% off,
almost all wider than tall. Many of those are CD front inserts at about
600×535. Those numbers are why this is "a select few", not a batch over
everything.

**Built 2026-11-19:** the `Square` batch framing and the list scope that
chooses "a select few". Still to build:

- **A draggable square crop in the editor** for a single image, saved in
  the original format. A centre crop is wrong for some covers. For example,
  a CD insert with the hinge strip down one side needs the crop taken from
  the other side. Overwrite already writes a JPEG back as JPEG, so the
  crop only needs the editor's side.

**Open.**

- Whether `Square` should refuse an image beyond some limit (say 25% off),
  where a centre crop would remove a large part of the art, or only warn in
  the dry run.

**Workflow it slots into.** Dry run, then run *Into the library*, which
keeps the originals (the Cap runs of 2026-11-13 did the same by hand with
`swap-covers.py`). In ndisc,
"Local covers vs Blossom" then offers the squared covers for upload. They are
changed since publishing but not a larger copy of the same shape, so they
will not be pre-ticked and need ticking by hand.
