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

Suggested build order: list scope + dimension filters, then ndisc's export,
then Square, then the library write-back.

## List scope and dimension filters

**Why.** Whether a cover is non-square, too large or too small is a fact about
the file, and ncover already reads image headers fast (`image_dims`, built for
Cap). ncover can find these covers itself, in any folder, without ndisc.

**What.**

- **Scope `From a list…`**: a text file with one release folder per line. Any
  further tab-separated columns are ignored by the walk but shown in the Files
  list. It feeds `walk_scope`, so the survey, the count and the run stay one
  walk ("the count is a promise").
- **Filters** beside the name filter: non-square by more than X%, long side
  over N px, long side under N px. They join the walk's selection, not a
  separate pass, so the survey line, the Files list, the samples and the Run
  count all agree with them.
- **Files list rows** show each image's dimensions and gain an **Open folder**
  action, for replacing a cover by hand.

## ndisc: export the "Local covers vs Blossom" result (ndisc repo)

**Why.** Only ndisc knows what is published, so only it can say which covers
differ from Blossom. ncover should not grow a database or read ndisc's.

**What.** An **Export list** button on the Local covers vs Blossom result. It
writes a text file with one release folder per line, with extra tab-separated
columns: artist — title, disk size, published size, and the kind of
difference (shrunk, published copy is larger, different picture). ncover's
From a list… scope reads it. The key is the release **folder**, as in ndisc's
`published.json`, not a database id, so ncover stays a reader of what ndisc
exports, and the file is usable by hand.

This item belongs to ndisc; it is recorded here because the loop is ncover's
to make whole.

## Write results into the library, keeping originals

**Why.** On 2026-11-13 the Cap runs went to an output folder and a desktop
script (`swap-covers.py`) moved each original aside and the new file into
place, logging every swap. That step belongs in ncover, so the loop runs
inside the two apps.

**What.** A run option that, after a successful run, swaps each output into the
source library. Each original moves to a chosen originals folder under the
same relative path. Every swap is logged (source, original kept, written),
and an Undo restores from the log. It refuses to touch a file whose original
slot is already taken. Batch's rule that it "never writes over its source"
then becomes "never without keeping the original", and the CLAUDE.md
write-path notes must say so.

## Square: crop chosen covers to square (planned 2026-11-13)

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

**What.**

1. **Batch framing `Square`**, beside Cover / Fit / Cap. It crops each image to
   a square of its shorter side, taking the centre along the long side, then
   applies the size limit as Cap does (shrink only, `area_resample`). It keeps
   the format (JPEG q92 stays JPEG, PNG stays PNG), keeps the file name, and
   writes nothing for an image that is already square. Like Cap, its count
   ("Run (N of T to square)"), samples and dry run come from the same header
   read and rule as the run.
2. **Batch scope `From a list…`**: a text file with one release folder per
   line (a second tab-separated column is ignored, so `review-folders.txt`
   style lists work). It feeds the same `walk_scope`, so the name filter and
   the survey work unchanged. This is how "a select few" is chosen.
3. **Later: a draggable square crop in the editor** for a single image, saved
   in the original format. A centre crop is wrong for some covers. For
   example, a CD insert with the hinge strip down one side needs the crop
   taken from the other side. Today the editor always saves PNG (the disc
   mask needs alpha), so saving JPEG from the editor is its own change and
   comes after 1 and 2.

**Open.**

- Whether `Square` should refuse an image beyond some limit (say 25% off),
  where a centre crop would remove a large part of the art, or only warn in
  the dry run.
- Whether the macOS sibling gets the same feature. It would be a port of the
  idea and its tests, as with the rest of the parity plan, not shared code.

**Workflow it slots into.** Output to a separate folder, dry run, run, then
swap into the library keeping the originals. That is the same flow as the Cap
runs of 2026-11-13 (`swap-covers.py`, originals kept beside it). In ndisc,
"Local covers vs Blossom" then offers the squared covers for upload. They are
changed since publishing but not a larger copy of the same shape, so they
will not be pre-ticked and need ticking by hand.
