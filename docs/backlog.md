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
(unreleased). Next: Convert, then the dimension filters.

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

## Dimension filters and the Files list

**Built 2026-11-19:** the `From a list…` scope, reading ndisc's Export list
(folder · cover file · artist — title · disk WxH · published WxH · format ·
kinds · actions, tab-separated, `-` = empty). The framing picks the lines
(`ncover:cap`, `ncover:square`), only the named cover file is taken, and the
survey counts the lines left out. Still to build:

- **Convert (PNG → JPEG)**, a job of its own for the `ncover:convert` lines.
  Order when a cover needs several jobs: square, then cap; convert joins
  whichever run writes last.
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
