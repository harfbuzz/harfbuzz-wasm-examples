use harfbuzz_wasm::Font;
use kurbo::{Affine, BezPath, ParamCurve, PathEl, Point, QuadBez, Rect, Shape};

// Return a slightly scaled-up copy of a glyph's outline.
// We create this slightly bigger copy of the glyphs so that
// when we do collision tests between glyphs, they have a bit
// of breathing space around them.
pub fn get_scaled_outline(font: &Font, glyph: u32) -> Vec<BezPath> {
    let mut paths = font.get_outline(glyph);
    // Find a bounding box which encompasses all the paths.
    if let Some(big_bounds) = bounding_box(&paths) {
        // Scale up 10% around the center.
        let center_vec = big_bounds.center().to_vec2();
        let affine = Affine::translate(center_vec * -1.0)
            * Affine::scale(1.12)
            * Affine::translate(center_vec);
        for path in paths.iter_mut() {
            apply_affine(path, affine);
        }
    }
    paths
}

// This is path.apply_affine(affine), spelled out so that it is
// compiled into our loop: kurbo's calls a function for every
// element, to do the same sums.
#[inline]
fn apply_affine(path: &mut BezPath, affine: Affine) {
    for el in path.elements_mut() {
        match el {
            PathEl::MoveTo(p) => *p = affine * *p,
            PathEl::LineTo(p) => *p = affine * *p,
            PathEl::QuadTo(p1, p2) => {
                *p1 = affine * *p1;
                *p2 = affine * *p2;
            }
            PathEl::CurveTo(p1, p2, p3) => {
                *p1 = affine * *p1;
                *p2 = affine * *p2;
                *p3 = affine * *p3;
            }
            PathEl::ClosePath => {}
        }
    }
}

// A box with nothing in it yet.
const NOTHING: Rect = Rect::new(
    f64::INFINITY,
    f64::INFINITY,
    f64::NEG_INFINITY,
    f64::NEG_INFINITY,
);

// Make a box big enough for one more point. Rect::union_pt() says the
// same of ordinary numbers, but it gets there through fmin() and
// fmax(), which are functions to call here, four times for a point.
#[inline]
fn include(bounds: &mut Rect, p: Point) {
    if p.x < bounds.x0 {
        bounds.x0 = p.x;
    }
    if p.x > bounds.x1 {
        bounds.x1 = p.x;
    }
    if p.y < bounds.y0 {
        bounds.y0 = p.y;
    }
    if p.y > bounds.y1 {
        bounds.y1 = p.y;
    }
}

// The union of the bounding boxes of all the paths, as kurbo works
// them out: None if there are no paths.
fn kurbo_bounding_box(paths: &[BezPath]) -> Option<Rect> {
    paths
        .iter()
        .map(|x| x.bounding_box())
        .reduce(|a, b| a.union(b))
}

// The same box as kurbo_bounding_box(), found in one pass over lines
// and quadratic curves, which is what a glyph is made of here. (The
// same numbers, that is. A zero can come out with the other sign,
// which the center of the box does not care about.) Kurbo makes a
// box for every segment and merges them one by one; this just keeps
// the smallest and largest x and y of the points those boxes are made
// from: the two ends of each segment, as kurbo evaluates them, and
// the points where a curve turns back in x or in y.
//
// This holds for coordinates that are ordinary numbers: finite, and
// far from overflowing when added or multiplied by two. For any
// others, kurbo is asked.
fn bounding_box(paths: &[BezPath]) -> Option<Rect> {
    if paths.is_empty() {
        return None;
    }
    let mut bounds = NOTHING;
    let mut add = |p: Point| include(&mut bounds, p);
    // The shortcuts below hold for ordinary numbers. We add up the
    // squares of all the coordinates as we go: if anything is infinite,
    // not a number, or so big that sums of them could be, so is this.
    let mut squares = 0.0;
    for path in paths {
        // Kurbo's rules for cutting a path into segments: a segment
        // goes from the last point to the element's own, and closing
        // the path adds a line back to where it started, if it is not
        // there already. A path begins with a MoveTo, or else kurbo
        // can deal with it.
        let mut els = path.elements().iter();
        let (mut start, mut last) = match els.next() {
            Some(&PathEl::MoveTo(p)) => (p, p),
            Some(_) => return kurbo_bounding_box(paths),
            None => (Point::ZERO, Point::ZERO),
        };
        squares += start.x * start.x + start.y * start.y;
        // Whether the last point is already in the box.
        let mut last_added = false;
        let mut segments = false;
        for el in els {
            match *el {
                PathEl::MoveTo(p) => {
                    squares += p.x * p.x + p.y * p.y;
                    start = p;
                    last = p;
                    last_added = false;
                }
                PathEl::LineTo(p) => {
                    squares += p.x * p.x + p.y * p.y;
                    if !last_added {
                        add(last);
                    }
                    // Kurbo evaluates the line at 0 and at 1 for its
                    // ends: 0 gives the last point, but 1 gives this,
                    // which rounding can leave a hair away from `p`.
                    let end = Point::new(last.x + (p.x - last.x), last.y + (p.y - last.y));
                    add(end);
                    last = p;
                    last_added = end == p;
                    segments = true;
                }
                PathEl::QuadTo(p1, p2) => {
                    squares += p1.x * p1.x + p1.y * p1.y + p2.x * p2.x + p2.y * p2.y;
                    // Evaluating a curve at 0 and at 1 does give its
                    // two end points.
                    if !last_added {
                        add(last);
                    }
                    add(p2);
                    // The curve turns back in x where its derivative
                    // in x is zero: at t = -d0 / (d1 - d0), if that is
                    // between 0 and 1. It cannot be unless d0 and d1
                    // have opposite signs, so mostly we need not ask.
                    let d0 = p1 - last;
                    let d1 = p2 - p1;
                    if (d0.x < 0.0 && d1.x > 0.0) || (d0.x > 0.0 && d1.x < 0.0) {
                        let t = -d0.x / (d1.x - d0.x);
                        if t > 0.0 && t < 1.0 {
                            add(QuadBez::new(last, p1, p2).eval(t));
                        }
                    }
                    if (d0.y < 0.0 && d1.y > 0.0) || (d0.y > 0.0 && d1.y < 0.0) {
                        let t = -d0.y / (d1.y - d0.y);
                        if t > 0.0 && t < 1.0 {
                            add(QuadBez::new(last, p1, p2).eval(t));
                        }
                    }
                    last = p2;
                    last_added = true;
                    segments = true;
                }
                PathEl::ClosePath => {
                    if last != start {
                        if !last_added {
                            add(last);
                        }
                        let end =
                            Point::new(last.x + (start.x - last.x), last.y + (start.y - last.y));
                        add(end);
                        last = start;
                        last_added = end == start;
                        segments = true;
                    }
                }
                // Cubic curves we leave to kurbo.
                PathEl::CurveTo(..) => return kurbo_bounding_box(paths),
            }
        }
        if !segments {
            // Kurbo's box for a path without segments is a point at
            // the origin.
            add(Point::ZERO);
        }
    }
    if !(squares < f64::INFINITY) {
        return kurbo_bounding_box(paths);
    }
    Some(bounds)
}
