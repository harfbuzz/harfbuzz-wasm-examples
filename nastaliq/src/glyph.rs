use crate::outline::{get_scaled_outline, include, NOTHING};
use harfbuzz_wasm::{Buffer, BufferItem, CGlyphExtents, CGlyphInfo, CGlyphPosition, Font};
use kurbo::{flatten, Affine, BezPath, PathEl, Point, Rect};
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
    // The same goes for the outline as collision tests want it.
    corners: OnceCell<Option<Corners>>,
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
            corners: OnceCell::new(),
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

// Collision tests flatten outlines into line segments, with a
// tolerance so loose (fifty times the font's scale) that every curve
// of a glyph comes out as one line, from its start to its end. When
// we can be sure of that for an outline, wherever its glyph is put,
// we need not ask kurbo to flatten it every time: the flattened path
// is its corners, the points where its lines and curves end, moved to
// where the glyph is. These are the corners, where the glyph was drawn.
//
// "Sure" needs three things, and where one is missing we flatten the
// long way:
//
// - Each path is a MoveTo, then lines and quadratic curves, then a
//   ClosePath. This is how glyf outlines come.
//
// - Every coordinate is a multiple of 2^-62, and no bigger than 2^40.
//   (Any number from 2^-10 up is such a multiple.) A glyph is moved by
//   whole numbers, so everywhere it is put its coordinates, their
//   differences and the products of two differences stay on a grid,
//   and the sums in kurbo's estimate and in cross() neither overflow
//   nor underflow: each is right to a few parts in 2^53.
//
// - The two legs of a curve's control polygon are no longer than
//   1/32 of the tolerance. Kurbo estimates how many lines a curve
//   needs from where it sits on the parabola y = x^2: with the ends at
//   x0 and x2 and the parabola scaled by s, the count is
//   ceil(0.5 * val / sqrt(tolerance)), where val is
//   |a(x2) - a(x0)| * sqrt(s) when the ends are on one side of the
//   vertex, and sqrt(tolerance) * |a(x2) - a(x0)| / a(sqrt(tolerance / s))
//   when the vertex is between them; a(x) is its approximate integral
//   x / (0.33 + (0.67^4 + x^2 / 4)^(1/4)). It has |x0| * cross <=
//   leg01 * |dd| and |x2| * cross <= leg12 * |dd|, and s = cross /
//   (|dd| * |x2 - x0|), for its cross product and second difference dd.
//   Since a() rises no faster than x, val^2 <= leg01 + leg12 in the
//   first case, far below 4 * tolerance. In the second, s * x0^2 <=
//   leg01 and s * x2^2 <= leg12 make |x0| and |x2| at most a quarter
//   of sqrt(tolerance / s) once the legs are within tolerance / 16,
//   and a(x / 4) <= a(x) / 2, so val <= sqrt(tolerance). Either way
//   the count is one. The rounding of a few parts in 2^53 at each
//   step, and of moving the glyph, fits in the factor of two that
//   1/32 leaves.
#[derive(Debug)]
struct Corners {
    // The corners of all the paths, one path after another...
    points: Vec<Point>,
    // ...and how many of them belong to each path.
    lengths: Vec<usize>,
    // The box around them, the longest a segment between two of
    // them gets in x or in y, and the biggest coordinate.
    bounds: Rect,
    reach: f64,
    biggest: f64,
}

impl Corners {
    fn of(paths: &[BezPath], tolerance: f64) -> Option<Corners> {
        // The font's scale is a whole number: this says it is positive.
        if !(tolerance >= 50.0) {
            return None;
        }
        // Is this a multiple of 2^-62 no bigger than 2^40? From
        // 2^-10 up, every number is such a multiple.
        let plain = |v: f64| {
            let scaled = v * 4611686018427387904.0;
            v.abs() <= 1099511627776.0 && (v.abs() >= 0.0009765625 || scaled.trunc() == scaled)
        };
        let longest = tolerance / 32.0;
        let mut corners = Corners {
            points: Vec::with_capacity(paths.iter().map(|path| path.elements().len()).sum()),
            lengths: Vec::with_capacity(paths.len()),
            bounds: NOTHING,
            reach: 0.0,
            biggest: 0.0,
        };
        // A segment between two corners: note how far it reaches.
        let join = |reach: &mut f64, from: Point, to: Point| {
            for long in [(to.x - from.x).abs(), (to.y - from.y).abs()] {
                if long > *reach {
                    *reach = long;
                }
            }
        };
        for path in paths {
            let (first, rest) = path.elements().split_first()?;
            let (last, middle) = rest.split_last()?;
            let (&PathEl::MoveTo(start), PathEl::ClosePath) = (first, last) else {
                return None;
            };
            if !(plain(start.x) && plain(start.y)) {
                return None;
            }
            let begin = corners.points.len();
            corners.points.push(start);
            include(&mut corners.bounds, start);
            let mut from = start;
            for el in middle {
                let to = match *el {
                    PathEl::LineTo(p) => p,
                    PathEl::QuadTo(p1, p2) => {
                        let legs = [
                            (p1.x - from.x).abs() + (p1.y - from.y).abs(),
                            (p2.x - p1.x).abs() + (p2.y - p1.y).abs(),
                        ];
                        if !(plain(p1.x) && plain(p1.y) && legs[0] <= longest && legs[1] <= longest)
                        {
                            return None;
                        }
                        p2
                    }
                    _ => return None,
                };
                if !(plain(to.x) && plain(to.y)) {
                    return None;
                }
                corners.points.push(to);
                include(&mut corners.bounds, to);
                join(&mut corners.reach, from, to);
                from = to;
            }
            // The last corner is joined to the first.
            join(&mut corners.reach, from, start);
            corners.lengths.push(corners.points.len() - begin);
        }
        // The biggest coordinate is on the box.
        let bounds = corners.bounds;
        for big in [
            bounds.x0.abs(),
            bounds.y0.abs(),
            bounds.x1.abs(),
            bounds.y1.abs(),
        ] {
            if big > corners.biggest {
                corners.biggest = big;
            }
        }
        Some(corners)
    }

    // The segments between the corners that have a part within
    // `margin` of the box `near`, for a glyph at `at`: as segments
    // for cross() in `segments`, and their own box, where the glyph
    // was drawn, as the result. `near` is where the glyph was drawn
    // too. A segment is left out only when both its ends are beyond
    // the same side of the box by more than the margin.
    fn near(&self, near: Rect, margin: f64, at: (f64, f64), segments: &mut Vec<Segment>) -> Rect {
        let (x0, x1) = (near.x0 - margin, near.x1 + margin);
        let (y0, y1) = (near.y0 - margin, near.y1 + margin);
        let mut found = NOTHING;
        let mut begin = 0;
        for length in &self.lengths {
            let path = &self.points[begin..begin + length];
            begin += length;
            // The last corner is joined to the first.
            let mut from = path[length - 1];
            for &to in path {
                if !((from.x < x0 && to.x < x0)
                    || (from.x > x1 && to.x > x1)
                    || (from.y < y0 && to.y < y0)
                    || (from.y > y1 && to.y > y1))
                {
                    include(&mut found, from);
                    include(&mut found, to);
                    // The glyph's paths are positioned by adding
                    // where it is to every point.
                    segments.push(Segment::new(
                        Point::new(from.x + at.0, from.y + at.1),
                        Point::new(to.x + at.0, to.y + at.1),
                    ));
                }
                from = to;
            }
        }
        found
    }
}

impl GlyphShape {
    // The corners of the outline, if the collision tests can use them.
    fn corners(&self, font: &Font, tolerance: f64) -> Option<&Corners> {
        self.corners
            .get_or_init(|| Corners::of(self.outline(font), tolerance))
            .as_ref()
    }
}

// How far apart in the buffer two glyphs can be for us to remember
// whether they collide: from NEAR glyphs before to NEAR - 1 after.
// The dot avoidance loops look no further than that.
const NEAR: usize = 8;

// What we remember about a pair of glyphs.
const NOT_ASKED: u8 = 0;
const APART: u8 = 1;
const COLLIDING: u8 = 2;

// Collision detector for the glyphs of a buffer. It remembers what
// it has worked out until a glyph moves (tell it with moved()): the
// bounding box of every glyph and whether a glyph collides with each
// of those near it.
pub struct Collider<'a> {
    shapes: &'a [GlyphShape],
    font: &'a Font,
    // How finely to flatten the outlines.
    tolerance: f64,
    bounds: Vec<Bounds>,
    // For each glyph, 2 * NEAR answers: one for each glyph near it.
    answers: Vec<u8>,
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
            answers: vec![NOT_ASKED; glyphs.len() * 2 * NEAR],
            mine: vec![],
            theirs: vec![],
            points: vec![],
        }
    }

    // Glyph number `ix` of the buffer has changed position.
    pub fn moved(&mut self, ix: usize, glyph: &GulzarGlyph) {
        self.bounds[ix] = Bounds::of(glyph, self.shapes[glyph.shape].extents(self.font));
        // Forget what we said about it and the glyphs near it...
        self.answers[ix * 2 * NEAR..(ix + 1) * 2 * NEAR].fill(NOT_ASKED);
        // ...and what we said about the glyphs it is near to and it.
        let first = ix.saturating_sub(NEAR - 1);
        let end = (ix + NEAR + 1).min(self.bounds.len());
        for other in first..end {
            self.answers[other * 2 * NEAR + (ix + NEAR - other)] = NOT_ASKED;
        }
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
        // If `j` is near `i`, we may have looked already, and if
        // we haven't, we will have the next time.
        let near = (j + NEAR).wrapping_sub(i);
        if near >= 2 * NEAR {
            return self.test(glyphs, i, j);
        }
        let answer = i * 2 * NEAR + near;
        if self.answers[answer] == NOT_ASKED {
            self.answers[answer] = if self.test(glyphs, i, j) {
                COLLIDING
            } else {
                APART
            };
        }
        self.answers[answer] == COLLIDING
    }

    // Look at the outlines of glyphs number `i` and `j`: do they cross?
    fn test(&mut self, glyphs: &[GulzarGlyph], i: usize, j: usize) -> bool {
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
        let my_corners = self.shapes[me.shape].corners(self.font, self.tolerance);
        let their_corners = self.shapes[them.shape].corners(self.font, self.tolerance);
        if let (Some(mine), Some(theirs)) = (my_corners, their_corners) {
            // The outlines are their corners. We only need the
            // segments of each that come near the other.
            //
            // How near? cross() (kurbo's test) says two segments
            // cross when |det| >= 1e-9 and t and u, two quotients
            // by det, are in [-1e-9, 1 + 1e-9] and [0, 1]. The
            // three numerators are each a difference of two products
            // of differences of coordinates, and for those there is
            // an identity: det * e + u_top * d - t_top * l = 0, with e
            // from the start of one segment to the start of the
            // other, and d and l the two segments. So the point of
            // one at u and the point of the other at t are apart, in
            // x and in y, by what rounding puts into that sum,
            // divided by det: at most 12 * 2^-53 * |e| * |d| * |l| /
            // 1e-9, less than 1.34e-6 * |e| * |d| * |l|, and by 1e-9 * |l|
            // more for the slack in t. Segments further apart than
            // that, in x or in y, cannot cross: for long segments
            // rounding does make kurbo see crossings between
            // segments that lie along one line some way apart, and
            // this bound is what keeps those.
            //
            // The sizes: of |e|, the box around both outlines; of
            // |d| and |l|, the reach of the outlines; and all of
            // them, and the margin, padded for the rounding of
            // moving the glyphs, a few times 2^-53 of the biggest
            // coordinate there is.
            let shift = (their_place.0 - my_place.0, their_place.1 - my_place.1);
            let slack = 64.0
                * (f64::EPSILON / 2.0)
                * (mine.biggest.max(theirs.biggest)
                    + my_place
                        .0
                        .abs()
                        .max(my_place.1.abs())
                        .max(their_place.0.abs())
                        .max(their_place.1.abs()));
            let mut both = mine.bounds;
            include(
                &mut both,
                Point::new(theirs.bounds.x0 + shift.0, theirs.bounds.y0 + shift.1),
            );
            include(
                &mut both,
                Point::new(theirs.bounds.x1 + shift.0, theirs.bounds.y1 + shift.1),
            );
            let size = if both.width() > both.height() {
                both.width()
            } else {
                both.height()
            } + slack;
            let (my_reach, their_reach) = (mine.reach + slack, theirs.reach + slack);
            let margin = 1.34e-6 * size * my_reach * their_reach
                + 1.01e-9 * (size + my_reach + their_reach)
                + slack;
            // Their segments near my outline; then my segments near
            // those. Each box is moved to where the other glyph was
            // drawn.
            let found = theirs.near(
                Rect::new(
                    mine.bounds.x0 - shift.0,
                    mine.bounds.y0 - shift.1,
                    mine.bounds.x1 - shift.0,
                    mine.bounds.y1 - shift.1,
                ),
                margin,
                their_place,
                &mut self.theirs,
            );
            if self.theirs.is_empty() {
                return false;
            }
            mine.near(
                Rect::new(
                    found.x0 + shift.0,
                    found.y0 + shift.1,
                    found.x1 + shift.0,
                    found.y1 + shift.1,
                ),
                margin,
                my_place,
                &mut self.mine,
            );
        } else {
            // Flatten them the long way, all of them.
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
        }
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
