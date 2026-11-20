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
