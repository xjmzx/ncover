# n.cover

<img align="right" width="160" src="https://raw.githubusercontent.com/xjmzx/ncover/main/extra/n.circle.png">

[![GitHub release](https://img.shields.io/github/v/release/xjmzx/ncover.svg)](https://github.com/xjmzx/ncover/releases)
[![dependency status](https://deps.rs/repo/github/xjmzx/ncover/status.svg)](https://deps.rs/repo/github/xjmzx/ncover)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

**n.cover** is a GTK4 desktop app for picking colours and building **cover /
label artwork**. It began as a wrapper around the [`xcolor`](#the-bundled-xcolor-cli)
X11 screen picker and grew into a small artwork tool. Part of the `n.*` suite.

**Colour**

- Pick any colour on screen (via the bundled `xcolor` CLI) — HEX / RGB / HSL,
  copied to the clipboard.
- Recent colours collect into a compact **History palette**.
- **Palettes:** import `.gpl` / `.json`, export to GPL / CSS / JSON, named
  swatches, pin up to three to the Picker, plus a dedicated Palettes view.

**Artwork**

- Open **PNG / SVG / JPEG / WebP**, or **Browse** a folder and scroll it with the
  arrow keys.
- Click any pixel to pick it; build a palette from an image (quantised) or from
  an SVG (its *declared* colours, with fonts and text).
- Place a source on a square canvas (200–1000 px) with drag, zoom, snapping and a
  grid.
- **Build:** a blank canvas, Invert, centred squares, and a disc / label mask
  (alpha / white / colour / gradient corners) — the steps stack.
- A **Source / Result** rail to compare the raw image against what you built.
- Save as PNG, or **Overwrite** the original PNG in place.
- **Batch** the same recipe over a folder, or over an ndisc-published
  discography — with a Preview contact sheet and a dry run. It never writes over
  your source.

Output is always PNG (the disc mask needs alpha).

## Install

`n.cover` needs GTK 4. It shells out to the `xcolor` CLI, which is built
alongside it and needs [xcb](https://xcb.freedesktop.org) and
[Xlib](https://www.x.org/wiki/).

``` shell
make gui                              # build target/release/ncover
make install-gui PREFIX=$HOME/.local  # user install (~/.local/bin + .desktop)
```

`make install-all` additionally installs the `xcolor` CLI, its man page and
icons under `PREFIX` (default `/usr/local`). To build the latest from git:

``` shell
cargo install --git 'https://github.com/xjmzx/ncover.git'
```

## The bundled `xcolor` CLI

`n.cover` uses `xcolor` — the X11 screen picker this repository is forked from —
for its screen picks. It is also a capable standalone tool, and its own
documentation follows.

## Usage

Simply invoke the `xcolor` command to select a color. The selected color will be
printed to the standard output.

``` text
xcolor 0.5.0
Samuel Laurén <samuel.lauren@iki.fi>:Callum Osmotherly <acheronfail@gmail.com>
Lightweight color picker for X11

USAGE:
    xcolor [OPTIONS]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -c, --custom <FORMAT>                Custom output format
    -f, --format <NAME>                  Output format (defaults to hex) [possible values: hex, HEX, hex!, HEX!, plain,
                                         rgb]
    -P, --preview-size <PREVIEW_SIZE>    Size of preview, must be odd (defaults to 255)
    -S, --scale <SCALE>                  Scale of magnification (defaults to 8)
    -s, --selection <SELECTION>          Output to selection (defaults to clipboard) [possible values: primary,
                                         secondary, clipboard]
```

## Saving to Selection

By default, the selected color is printed to the standard output. By specifying
the `-s` flag, `xcolor` can be instructed to instead save the color to X11's
selection. The selection to use can be specified as an argument. Possible
selection values are `clipboard` (the default), `primary`, and `secondary`.

Because of the way selections work in X11, `xcolor` forks into background when
`-s` mode is used. This behavior can be disabled by defining `XCOLOR_FOREGROUND`
environment variable.

## Color Preview

The `-S` or `--scale` flag controls the upscaling (or zoom) of the preview. By
default it is set to `8` which indicates an 8x zoom level.

The `-P` or `--preview-size` flag controls the size of the preview in pixels. So
that the preview always has a center pixel this number must be odd, if an even
number is passed then it will be changed to the next odd number.

## Formatting

By default, the color values will be printed in lowercase hexadecimal format.
The output format can be changed using the `-f NAME` switch. Supported format
names are listed below:

| Format Specifier | Description                               | Example               | Custom Format Equivalent |
| ---------------- | ----------------------------------------- | --------------------- | ------------------------ |
| `hex`            | Lowercase hexadecimal (default)           | `#ff00ff`             | `#%{02hr}%{02hg}%{02hb}` |
| `HEX`            | Uppercase hexadecimal                     | `#00FF00`             | `#%{02Hr}%{02Hg}%{02Hb}` |
| `hex!`           | Compact lowercase hexadecimal<sup>1</sup> | `#fff`                | Not expressible          |
| `HEX!`           | Compact uppercase hexadecimal<sup>1</sup> | `#F0F`                | Not expressible          |
| `rgb`            | Decimal RGB                               | `rgb(255, 255, 255)`  | `rgb(%{r}, %{g}, %{b})`  |
| `plain`          | Decimal with semicolon separators         | `0;0;0`               | `%{r};%{g};%{b}`         |

**1**: The compact form refers to CSS three-letter color codes as specified by [CSS
Color Module Level 3](https://www.w3.org/TR/2018/PR-css-color-3-20180315/#rgb-color).
If the color is not expressible in three-letter form, the regular six-letter
form will be used.

## Custom Formats

The `-f` switch provides quick access to some commonly used formatting options.
However, if custom output formatting is desired, this can be achieved using the
`-c FORMAT` switch. The `FORMAT` parameter specifies a template for the output
and supports a simple template language.

`FORMAT` templates can contain special expansions that are written inside
`%{...}` blocks. These blocks will be expanded into color values according to
the specifiers defined inside the block. Here are some examples of valid format
strings and what they might translate to:

| Format String            | Example Output     |
| ------------------------ | ------------------ |
| `%{r}, %{g}, %{b}`       | `255, 0, 100`      |
| `Green: %{-4g}`          | `Green: ---7`      |
| `#%{02hr}%{02hg}%{02hb}` | `#00ff00`          |
| `%{016Br}`               | `0000000000000011` |

Expansion blocks in format strings always contain a channel specifier (`r` for
red, `g` for green, and `b` for blue). Additionally, they can contain an
optional number format specifier (`h` for lowercase hexadecimal, `H` for
uppercase hexadecimal, `o` for octal, `B` for binary, and `d` for decimal) and
an optional padding specifier consisting of a character to use for padding and
the length the string should be padded to. We can use these rules to decode the
above example string:

``` text
  %{016Br}
    | |||
    | ||`- Channel (red)
    | |`-- Number format specifier (binary)
    | `--- Padding length (16)
    `----- Character to use for padding (0)
```

In the output, we get the contents of the red color channel formatted in binary
and padded with zeroes to be sixteen characters long.

## Issues

Bugs & Issues should be reported at [GitHub](https://github.com/xjmzx/ncover/issues).

## Credits & license

`n.cover` and the modernised CLI are maintained by **xjmzx**. The `xcolor` CLI
was originally written by **Samuel Laurén**
([`Soft/xcolor`](https://github.com/Soft/xcolor)), with contributions from Callum
Osmotherly and others. MIT-licensed, as upstream.
