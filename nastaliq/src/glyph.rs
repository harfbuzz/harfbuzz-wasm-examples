use crate::outline::get_scaled_outline;
use harfbuzz_wasm::{Buffer, BufferItem, CGlyphExtents, CGlyphInfo, CGlyphPosition, Font};
use kurbo::{flatten, Affine, BezPath, PathEl, Point};
use std::cell::OnceCell;

// The kinds of glyph we treat differently. A glyph can be of several kinds.
pub const DOT_BELOW: u8 = 1 << 0;
pub const DOT_ABOVE: u8 = 1 << 1;
pub const BARI_YE: u8 = 1 << 2;
pub const INIT: u8 = 1 << 3;
pub const ISOL: u8 = 1 << 4;
pub const FINA: u8 = 1 << 5;
pub const SPACE: u8 = 1 << 6;

// What we need to know about a glyph ID: what kinds of glyph its name
// makes it, its outline and its extents. Finding these out is expensive,
// and a buffer has the same few glyph IDs over and over, so we do it once
// for each glyph ID and let all the glyphs with that ID share the result.
#[derive(Debug)]
pub struct GlyphShape {
    pub kinds: u8,
    // Not every glyph gets asked for its outline or its extents,
    // so we only get them from the host the first time somebody does.
    glyph: u32,
    outline: OnceCell<Vec<BezPath>>,
    extents: OnceCell<CGlyphExtents>,
}

impl GlyphShape {
    // These rules will obviously need to be customized
    // for each font, but maybe in a production Nastaliq shaper
    // they would be driven from data in a font table.
    pub fn new(glyph: u32, name: &str) -> Self {
        let mut kinds = 0;
        if name.ends_with("BelowNS") || name.ends_with("HehCommaNS") {
            kinds |= DOT_BELOW;
        }
        if name.ends_with("AboveNS") || name.ends_with("FathaNS") {
            kinds |= DOT_ABOVE;
        }
        // The rest can be anywhere in the name. We look at each place in
        // the name once, for all of them, rather than search the name
        // five times: this is what name.contains("Ini") and so on say.
        let bytes = name.as_bytes();
        for at in 0..bytes.len() {
            let rest = &bytes[at..];
            match bytes[at] {
                b'Y' if rest.starts_with(b"YehBarreeFin") => kinds |= BARI_YE,
                b'I' if rest.starts_with(b"Ini") => kinds |= INIT,
                b'S' if rest.starts_with(b"Sep") => kinds |= ISOL,
                b'F' if rest.starts_with(b"Fin") => kinds |= FINA,
                b's' if rest.starts_with(b"space") => kinds |= SPACE,
                _ => {}
            }
        }
        Self {
            kinds,
            glyph,
            outline: OnceCell::new(),
            extents: OnceCell::new(),
        }
    }

    // The glyph's outline, scaled up a little.
    pub fn outline(&self, font: &Font) -> &Vec<BezPath> {
        self.outline
            .get_or_init(|| get_scaled_outline(font, self.glyph))
    }

    // The extents of the glyph, in its design position.
    pub fn extents(&self, font: &Font) -> &CGlyphExtents {
        self.extents
            .get_or_init(|| font.get_glyph_extents(self.glyph))
    }
}

// This is the standard glyph representation but with a few more
// handy fields.
#[derive(Debug)]
pub struct GulzarGlyph {
    pub codepoint: u32,
    // Which GlyphShape this glyph has, and a copy of its kinds.
    pub shape: usize,
    pub kinds: u8,
    pub cluster: u32,
    pub x_advance: i32,
    pub x_total_advance: i32,
    pub y_advance: i32,
    pub x_offset: i32,
    pub y_offset: i32,
    pub in_bari_ye: bool,
}

impl BufferItem for GulzarGlyph {
    fn from_c(info: CGlyphInfo, pos: CGlyphPosition) -> Self {
        Self {
            codepoint: info.codepoint,
            shape: 0,
            kinds: 0,
            cluster: info.cluster,
            x_advance: pos.x_advance,
            x_total_advance: 0,
            y_advance: pos.y_advance,
            x_offset: pos.x_offset,
            y_offset: pos.y_offset,
            in_bari_ye: false,
        }
    }
    fn to_c(self) -> (CGlyphInfo, CGlyphPosition) {
        let info = CGlyphInfo {
            codepoint: self.codepoint,
            cluster: self.cluster,
            mask: 0,
            var1: 0,
            var2: 0,
        };
        let pos = CGlyphPosition {
            x_advance: self.x_advance,
            y_advance: self.y_advance,
            x_offset: self.x_offset,
            y_offset: self.y_offset,
            var: 0,
        };
        (info, pos)
    }
}

impl GulzarGlyph {
    pub fn is_dot_below(&self) -> bool {
        self.kinds & DOT_BELOW != 0
    }

    pub fn is_dot_above(&self) -> bool {
        self.kinds & DOT_ABOVE != 0
    }

    pub fn is_bari_ye(&self) -> bool {
        self.kinds & BARI_YE != 0
    }

    pub fn is_init(&self) -> bool {
        self.kinds & INIT != 0
    }

    pub fn is_isol(&self) -> bool {
        self.kinds & ISOL != 0
    }

    pub fn is_fina(&self) -> bool {
        self.kinds & FINA != 0
    }

    pub fn is_space(&self) -> bool {
        self.kinds & SPACE != 0
    }
}
pub type GulzarBuffer = Buffer<GulzarGlyph>;

// This is the bounding box of a *positioned* glyph, i.e. including
// the full running-total x advance.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Bounds {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}

impl Bounds {
    fn of(glyph: &GulzarGlyph, extents: &CGlyphExtents) -> Self {
        let bl_x = extents.x_bearing + glyph.x_total_advance + glyph.x_offset;
        let bl_y = extents.y_bearing + extents.height + glyph.y_offset;
        let tr_x = bl_x + extents.width;
        let tr_y = bl_y - extents.height;
        Bounds {
            x0: bl_x.min(tr_x),
            y0: bl_y.min(tr_y),
            x1: bl_x.max(tr_x),
            y1: bl_y.max(tr_y),
        }
    }

    // Do the two boxes have any area in common?
    fn overlaps(&self, other: &Bounds) -> bool {
        self.x1.min(other.x1) > self.x0.max(other.x0)
            && self.y1.min(other.y1) > self.y0.max(other.y0)
    }
}

// A line segment of a flattened outline: where it starts, and how
// far it goes from there.
#[derive(Clone, Copy)]
struct Segment {
    x: f64,
    y: f64,
    dx: f64,
    dy: f64,
}

impl Segment {
    fn new(from: Point, to: Point) -> Self {
        Segment {
            x: from.x,
            y: from.y,
            dx: to.x - from.x,
            dy: to.y - from.y,
        }
    }
}

// Do two line segments cross? This is what kurbo's
// PathSeg::intersect_line() works out for a line segment `a` and a
// probe line `b`, sum for sum, but all we want is a yes or no, and we
// have the differences between the ends of the segments already.
fn cross(a: &Segment, b: &Segment) -> bool {
    const EPSILON: f64 = 1e-9;
    let det = b.dx * a.dy - b.dy * a.dx;
    if det.abs() < EPSILON {
        // Lines are coincident (or nearly so).
        return false;
    }
    // t = position on a
    let t = b.dx * (b.y - a.y) - b.dy * (b.x - a.x);
    let t = t / det;
    if !(-EPSILON..=(1.0 + EPSILON)).contains(&t) {
        return false;
    }
    // u = position on b
    let u = (a.x - b.x) * a.dy - (a.y - b.y) * a.dx;
    let u = u / det;
    (0.0..=1.0).contains(&u)
}

// Collision detector for the glyphs of a buffer. Still terribly
// inefficient, but it remembers the bounding box of every glyph
// until the glyph moves (tell it with moved()).
pub struct Collider<'a> {
    shapes: &'a [GlyphShape],
    font: &'a Font,
    // How finely to flatten the outlines.
    tolerance: f64,
    bounds: Vec<Bounds>,
    // The segments of the two glyphs being tested.
    mine: Vec<Segment>,
    theirs: Vec<Segment>,
    points: Vec<Point>,
}

impl<'a> Collider<'a> {
    pub fn new(glyphs: &[GulzarGlyph], shapes: &'a [GlyphShape], font: &'a Font, sx: i32) -> Self {
        Collider {
            shapes,
            font,
            tolerance: 50.0 * (sx as f64),
            bounds: glyphs
                .iter()
                .map(|glyph| Bounds::of(glyph, shapes[glyph.shape].extents(font)))
                .collect(),
            mine: vec![],
            theirs: vec![],
            points: vec![],
        }
    }

    // Glyph number `ix` of the buffer has changed position.
    pub fn moved(&mut self, ix: usize, glyph: &GulzarGlyph) {
        self.bounds[ix] = Bounds::of(glyph, self.shapes[glyph.shape].extents(self.font));
    }

    // Do glyphs number `i` and `j` of the buffer collide?
    #[inline(always)]
    pub fn collides(&mut self, glyphs: &[GulzarGlyph], i: usize, j: usize) -> bool {
        debug_assert!(
            self.bounds[i]
                == Bounds::of(&glyphs[i], self.shapes[glyphs[i].shape].extents(self.font))
                && self.bounds[j]
                    == Bounds::of(&glyphs[j], self.shapes[glyphs[j].shape].extents(self.font)),
            "a glyph moved without moved() being called"
        );
        // If the bounding boxes don't intersect, we can't collide.
        // That settles nearly every pair, so it is all that is done
        // where the question is asked.
        self.bounds[i].overlaps(&self.bounds[j]) && self.outlines_cross(glyphs, i, j)
    }

    // Do the outlines of glyphs number `i` and `j` cross?
    fn outlines_cross(&mut self, glyphs: &[GulzarGlyph], i: usize, j: usize) -> bool {
        let (me, them) = (&glyphs[i], &glyphs[j]);
        // Where the two glyphs are.
        let my_place = (
            (me.x_total_advance + me.x_offset) as f64,
            me.y_offset as f64,
        );
        let their_place = (
            (them.x_total_advance + them.x_offset) as f64,
            them.y_offset as f64,
        );
        self.mine.clear();
        self.theirs.clear();
        let my_paths = self.shapes[me.shape].outline(self.font);
        let their_paths = self.shapes[them.shape].outline(self.font);
        Self::flatten(
            my_paths,
            my_place,
            self.tolerance,
            &mut self.points,
            &mut self.mine,
        );
        Self::flatten(
            their_paths,
            their_place,
            self.tolerance,
            &mut self.points,
            &mut self.theirs,
        );
        // We could do line sweep or something here, but proof of concept...
        let theirs = &self.theirs;
        self.mine.iter().any(|a| theirs.iter().any(|b| cross(a, b)))
    }

    // The outline of a glyph at `place`, flattened into line segments.
    fn flatten(
        paths: &[BezPath],
        place: (f64, f64),
        tolerance: f64,
        points: &mut Vec<Point>,
        segments: &mut Vec<Segment>,
    ) {
        // Kurbo representations of the glyph's paths positioned
        // absolutely, but we don't need to keep them: they go
        // straight into the flattening.
        let affine = Affine::translate(place);
        for path in paths {
            points.clear();
            flatten(
                path.iter().map(|el| affine * el),
                tolerance,
                |el| match el {
                    PathEl::MoveTo(a) => points.push(a),
                    PathEl::LineTo(a) => points.push(a),
                    _ => {}
                },
            );
            // The segments go from each point to the next, and
            // from the last point back to the first.
            for (n, &from) in points.iter().enumerate() {
                let to = points[if n + 1 == points.len() { 0 } else { n + 1 }];
                segments.push(Segment::new(from, to));
            }
        }
    }
}
