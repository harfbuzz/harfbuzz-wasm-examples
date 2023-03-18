# WASM Shaper Demonstrations

This repository contains sources and binaries of example fonts using the Harfbuzz WASM Shaper.

## What do they demonstrate and why?

### gulzar

Urdu Nastaliq typography has long been seen as a challenge which pushes up against the limitations of OpenType Layout. John Hudson's TypeCon talk "Problems of Adjacency" ([slide 21-](http://tiro.com/John/TypeCon2014_Hudson_DECK.pdf#page=21)) illustrates some of the challenges of Nastaliq layout:

* Kerning between the bari ye glyph at the end of one word and the final glyph of the *previous word* is highly contextual, dependent entirely on the length of the word.
* Dots falling underneath the bari ye must be moved vertically to avoid the bari ye stroke, ideally to a consistent distance below the stroke regardless of their original Y offset. This is also highly contextual; trying to work out whether or not a glyph is above the bari ye depends entirely on the width and height of a variable number of glyphs following it.
* Dot positioning in general must be careful to avoid collisions both with other dots and with base characters.
* [Nastaliq kerning](https://simoncozens.github.io/nastaliq-autokerning/) is highly contextual dependent on the eventual Y position of the glyphs involved.

Each of these challenges *can* be largely overcome through approximation and enumerating a huge number of possible sequences; but the approximation is imperfect and there are still mistakes; and because the sequences are enumerated statically in advance, each of these challenges requires thousands - sometimes tens of thousands - of OpenType rules. As John says,

> As with the spacing problem, the quantity
and complexity of the lookups involved is huge, presenting challenges for workflows, font compilation, and performance...  But it is still a workaround: a convoluted hack to bypass the limitations of an inappropriate paradigm.

The `gulzar` directory contains a Nastaliq Urdu font which solves these problems; it has a Nastaliq-specific shaping engine which:

* computes the width of bari-ye sequences and adjusts the spacing of the previous word appropriately.
* computes the height of dots under bari ye and moves them underneath.
* detects and mitigates overlaps in glyph outlines, repositioning diacritics to avoid collisions.
* measures the height and contours of word endings and word beginnings to create even spacing.

All this is done at run-time by specifying the desired positioning behaviour directly, rather than enumerating the possibilities, meaning that the WASM font is one third the size of the OT implementation.

![](gulzar/comparison.png)


