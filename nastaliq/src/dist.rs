use crate::mindist::quad_quad_dist;
use core::ops::Range;
use harfbuzz_wasm::debug;
use kurbo::{BezPath, CubicBez, Line, ParamCurve, ParamCurveNearest, PathEl, PathSeg, Point, QuadBez};

// This is a Rust port of the Nastaliq kerning algorithm
// detailed at https://simoncozens.github.io/nastaliq-autokerning/
// It's horribly inefficient because I coded it in an excited
// hurry. Given two sets of paths, one for the left hand side
// and one for the right hand side, work out how many units to
// kern so that these paths are `target_distance` units apart.

// The paths of a glyph, and how far the glyph is moved from where its
// paths are. `key` says which glyph this is: a Kerner takes two of
// these with the same key to be the same glyph in the same place.
#[derive(Clone, Copy)]
pub struct Placed<'a> {
    pub key: usize,
    pub paths: &'a [BezPath],
    pub dx: f64,
    pub dy: f64,
}

// What determine_kern() works with. It is kept from one call to the
// next so that the memory is there already, and so are the segments of
// the last few glyphs: a glyph that is on the right of one kern is on
// the left of the next ones.
#[derive(Default)]
pub struct Kerner {
    kept: Vec<Kept>,
    clock: u32,
    left: Segments,
    right: Segments,
    pairs: Vec<PathPair>,
    near: Vec<(PointBox, usize)>,
}

// The segments of a glyph where it is, and when they were last used.
#[derive(Default)]
struct Kept {
    key: usize,
    used: u32,
    segments: Segments,
}

// How many glyphs are kept.
const KEPT: usize = 5;

impl Kerner {
    pub fn new() -> Self {
        Self::default()
    }

    // The left hand side is the paths of all the `left` glyphs, in that
    // order.
    pub fn determine_kern(
        &mut self,
        left: &[Placed],
        right: &Placed,
        target_distance: f32,
        max_tuck: f32,
        scale_factor: f32,
    ) -> f32 {
        // The left paths stay where they are, so take their segments, and
        // the points we compare segments by, once. We're going to slide
        // the right paths around until they fit.
        let (left_glyphs, right_glyph) = (left, right);
        let (left, right, pairs) = (&mut self.left, &mut self.right, &mut self.pairs);
        let near = &mut self.near;
        left.clear();
        for glyph in left_glyphs {
            left.append(keep(&mut self.kept, &mut self.clock, glyph));
        }
        right.clear();
        right.append(keep(&mut self.kept, &mut self.clock, right_glyph));
        pairs.clear();
        pairs.resize(left.paths.len() * right.paths.len(), PathPair::default());

        let mut minimum_possible = -1000.0 * scale_factor;
        let mut iterations = 0;
        let mut kern = 0.0;
        // This should probably be an Option<f32>: None or something
        // but like I said, hurry.`
        let mut min_distance = -9999.0;

        // Move them around up to ten times to get them within a target distance.
        while iterations < 10 && (target_distance - min_distance).abs() > 10.0 {
            // Work out how far they are away at closest point, work out the
            // kern, move the right hand paths and iterate.
            right.settle();
            if let Some(md) = path_distance(left, right, pairs, near) {
                min_distance = md;
                let this_kern = target_distance - min_distance;
                kern += this_kern;
                if kern < minimum_possible {
                    return minimum_possible;
                }

                // debug(&format!("Moving right paths another {:}", this_kern));
                right.move_by(this_kern as f64);
                iterations += 1;
            } else {
                return minimum_possible;
            }
        }
        kern
    }
}

// The segments of a glyph where it is: those kept from the last time,
// if it is one of the last few glyphs, or else worked out now, and kept
// instead of those of the glyph that has not been used for longest.
fn keep<'a>(kept: &'a mut Vec<Kept>, clock: &mut u32, glyph: &Placed) -> &'a Segments {
    *clock += 1;
    let mut oldest = 0;
    let mut found = None;
    for (i, one) in kept.iter().enumerate() {
        if one.key == glyph.key {
            found = Some(i);
            break;
        }
        if one.used < kept[oldest].used {
            oldest = i;
        }
    }
    let i = match found {
        Some(i) => i,
        None => {
            if kept.len() < KEPT {
                oldest = kept.len();
                kept.push(Kept::default());
            }
            let one = &mut kept[oldest];
            one.key = glyph.key;
            one.segments.clear();
            one.segments.add_glyph(glyph);
            oldest
        }
    };
    kept[i].used = *clock;
    &kept[i].segments
}

// How many segments of a path share a box.
const BOXED: usize = 3;

// A box around some points: the least x, the greatest x, the least y
// and the greatest y.
type PointBox = [f64; 4];

// The segments of the paths of some glyphs where the glyphs are, and
// the three points on each segment that segments are first compared by.
#[derive(Default)]
struct Segments {
    // The segments of all the paths, one path after the other.
    segs: Vec<PathSeg>,
    // The segments that close a subpath. There are hardly ever any.
    closers: Vec<usize>,
    // Whether there is a cubic segment.
    cubics: bool,
    // How far the segments still have to be moved along x.
    pending: Option<f64>,
    // The start, middle and end of each segment.
    points: Vec<[Point; 3]>,
    // A box around the points of every BOXED segments of a path: the
    // boxes of the first path, then those of the second, and so on.
    boxes: Vec<PointBox>,
    paths: Vec<Path>,
}

// One path: which segments and boxes are its own, the box around all
// its control points, and the box around the points of its segments.
#[derive(Clone)]
struct Path {
    segs: Range<usize>,
    boxes: Range<usize>,
    around: PointBox,
    points: PointBox,
}

impl Segments {
    fn clear(&mut self) {
        self.segs.clear();
        self.closers.clear();
        self.cubics = false;
        self.pending = None;
        self.points.clear();
        self.boxes.clear();
        self.paths.clear();
    }

    // Add the paths of a glyph, where the glyph is.
    fn add_glyph(&mut self, glyph: &Placed) {
        for path in glyph.paths {
            self.add_path(path, glyph.dx, glyph.dy);
        }
    }

    // Add the segments of some other paths, as they are.
    fn append(&mut self, other: &Segments) {
        let (segs, boxes) = (self.segs.len(), self.boxes.len());
        self.segs.extend_from_slice(&other.segs);
        self.closers
            .extend(other.closers.iter().map(|closer| closer + segs));
        self.cubics |= other.cubics;
        self.points.extend_from_slice(&other.points);
        self.boxes.extend_from_slice(&other.boxes);
        self.paths.extend(other.paths.iter().map(|path| Path {
            segs: path.segs.start + segs..path.segs.end + segs,
            boxes: path.boxes.start + boxes..path.boxes.end + boxes,
            around: path.around,
            points: path.points,
        }));
    }

    // Add a path moved by (dx, dy): its segments, their points and the
    // boxes around those.
    //
    // The segments are the ones that BezPath::segments() gives for the
    // moved path. We need to know which of them close a subpath,
    // though: such a segment is only there while the subpath's two ends
    // differ, and moving the path can make them the same point.
    fn add_path(&mut self, path: &BezPath, dx: f64, dy: f64) {
        let first_seg = self.segs.len();
        let first_box = self.boxes.len();
        // What Affine::translate((dx, dy)) does to a point.
        let moved = |p: Point| Point::new(p.x + dx, p.y + dy);
        // The box around the points of the last few segments, and how
        // many segments those are.
        let mut around = SmallBox::new();
        let mut boxed = 0;
        // The box around the control points that are not also points
        // the segments are compared by.
        let mut others = SmallBox::new();
        // Where the subpath started and where the last segment ended,
        // once there has been a path element.
        let mut start = Point::ZERO;
        let mut last = Point::ZERO;
        let mut begun = false;
        for el in path.elements() {
            let points = match *el {
                PathEl::MoveTo(p) => {
                    start = moved(p);
                    last = start;
                    begun = true;
                    continue;
                }
                PathEl::LineTo(p) => {
                    let p = moved(p);
                    if !begun {
                        (start, last, begun) = (p, p, true);
                    }
                    let l = Line::new(core::mem::replace(&mut last, p), p);
                    others.add(l.p0);
                    others.add(l.p1);
                    self.segs.push(PathSeg::Line(l));
                    [l.eval(0.0), l.eval(0.5), l.eval(1.0)]
                }
                PathEl::QuadTo(p1, p2) => {
                    let (p1, p2) = (moved(p1), moved(p2));
                    if !begun {
                        (start, last, begun) = (p2, p2, true);
                    }
                    let q = QuadBez::new(core::mem::replace(&mut last, p2), p1, p2);
                    others.add(p1);
                    self.segs.push(PathSeg::Quad(q));
                    // The first and the last control point of a
                    // quadratic are what QuadBez::eval() gives at 0 and
                    // at 1: there it adds the other points times zero to
                    // that one times one.
                    [q.p0, q.eval(0.5), q.p2]
                }
                PathEl::CurveTo(p1, p2, p3) => {
                    let (p1, p2, p3) = (moved(p1), moved(p2), moved(p3));
                    if !begun {
                        (start, last, begun) = (p3, p3, true);
                    }
                    let c = CubicBez::new(core::mem::replace(&mut last, p3), p1, p2, p3);
                    self.cubics = true;
                    self.segs.push(PathSeg::Cubic(c));
                    [c.eval(0.0), c.eval(0.5), c.eval(1.0)]
                }
                PathEl::ClosePath => {
                    if !begun || last == start {
                        continue;
                    }
                    let l = Line::new(core::mem::replace(&mut last, start), start);
                    others.add(l.p0);
                    others.add(l.p1);
                    self.closers.push(self.segs.len());
                    self.segs.push(PathSeg::Line(l));
                    [l.eval(0.0), l.eval(0.5), l.eval(1.0)]
                }
            };
            self.points.push(points);
            around.add(points[0]);
            around.add(points[1]);
            around.add(points[2]);
            boxed += 1;
            if boxed == BOXED {
                self.boxes.push(around.get());
                around = SmallBox::new();
                boxed = 0;
            }
        }
        if boxed > 0 {
            self.boxes.push(around.get());
        }
        let mut points = SmallBox::new();
        for around in &self.boxes[first_box..] {
            points.add(Point::new(around[0], around[2]));
            points.add(Point::new(around[1], around[3]));
        }
        // All the control points are in one of these two boxes.
        let points = points.get();
        others.add(Point::new(points[0], points[2]));
        others.add(Point::new(points[1], points[3]));
        self.paths.push(Path {
            segs: first_seg..self.segs.len(),
            boxes: first_box..self.boxes.len(),
            around: others.get(),
            points,
        });
    }

    // Move the segments along x, when they are needed again.
    fn move_by(&mut self, dx: f64) {
        self.pending = Some(dx);
    }

    // Move the segments if they were asked to, and with them their
    // points and boxes. Only x changes.
    fn settle(&mut self) {
        let Some(dx) = self.pending.take() else {
            return;
        };
        if !self.closers.is_empty() {
            return self.settle_all(dx);
        }
        for path in self.paths.iter_mut() {
            let segs = self.segs[path.segs.clone()].chunks_mut(BOXED);
            let points = self.points[path.segs.clone()].chunks_mut(BOXED);
            let boxes = self.boxes[path.boxes.clone()].iter_mut();
            path.points[0] = f64::INFINITY;
            path.points[1] = f64::NEG_INFINITY;
            for ((segs, points), around) in segs.zip(points).zip(boxes) {
                let (mut least, mut greatest) = (f64::INFINITY, f64::NEG_INFINITY);
                for (seg, points) in segs.iter_mut().zip(points.iter_mut()) {
                    // This is what applying Affine::translate((dx, 0.0))
                    // to the paths gives: x + dx, and y as it was.
                    match seg {
                        PathSeg::Line(l) => {
                            l.p0.x += dx;
                            l.p1.x += dx;
                            *points = [l.eval(0.0), l.eval(0.5), l.eval(1.0)];
                        }
                        PathSeg::Quad(q) => {
                            q.p0.x += dx;
                            q.p1.x += dx;
                            q.p2.x += dx;
                            points[0].x = q.p0.x;
                            points[1].x = q.eval(0.5).x;
                            points[2].x = q.p2.x;
                        }
                        PathSeg::Cubic(c) => {
                            c.p0.x += dx;
                            c.p1.x += dx;
                            c.p2.x += dx;
                            c.p3.x += dx;
                            *points = [c.eval(0.0), c.eval(0.5), c.eval(1.0)];
                        }
                    }
                    for p in points.iter() {
                        if p.x < least {
                            least = p.x;
                        }
                        if p.x > greatest {
                            greatest = p.x;
                        }
                    }
                }
                around[0] = least;
                around[1] = greatest;
                if least < path.points[0] {
                    path.points[0] = least;
                }
                if greatest > path.points[1] {
                    path.points[1] = greatest;
                }
            }
            // Adding to the least and to the greatest x of some points
            // gives the least and the greatest x of the moved points.
            path.around[0] += dx;
            path.around[1] += dx;
        }
    }

    // The same when there are segments that may have to go.
    fn settle_all(&mut self, dx: f64) {
        for (seg, points) in self.segs.iter_mut().zip(self.points.iter_mut()) {
            match seg {
                PathSeg::Line(l) => {
                    l.p0.x += dx;
                    l.p1.x += dx;
                }
                PathSeg::Quad(q) => {
                    q.p0.x += dx;
                    q.p1.x += dx;
                    q.p2.x += dx;
                }
                PathSeg::Cubic(c) => {
                    c.p0.x += dx;
                    c.p1.x += dx;
                    c.p2.x += dx;
                    c.p3.x += dx;
                }
            }
            *points = sample(seg);
        }
        self.drop_closed();
        self.boxes.clear();
        for path in self.paths.iter_mut() {
            let first_box = self.boxes.len();
            for points in self.points[path.segs.clone()].chunks(BOXED) {
                let mut around = SmallBox::new();
                for &p in points.iter().flatten() {
                    around.add(p);
                }
                self.boxes.push(around.get());
            }
            path.boxes = first_box..self.boxes.len();
            let mut points = SmallBox::new();
            for around in &self.boxes[path.boxes.clone()] {
                points.add(Point::new(around[0], around[2]));
                points.add(Point::new(around[1], around[3]));
            }
            path.points = points.get();
            path.around[0] += dx;
            path.around[1] += dx;
        }
    }

    // Drop the closing segments of subpaths whose two ends have become
    // the same point by moving them: BezPath::segments() has no segment
    // there any more.
    fn drop_closed(&mut self) {
        let mut k = 0;
        while k < self.closers.len() {
            let ix = self.closers[k];
            match self.segs[ix] {
                PathSeg::Line(l) if l.p0 == l.p1 => {}
                _ => {
                    k += 1;
                    continue;
                }
            }
            self.segs.remove(ix);
            self.points.remove(ix);
            self.closers.remove(k);
            for closer in self.closers[k..].iter_mut() {
                *closer -= 1;
            }
            for path in self.paths.iter_mut() {
                if path.segs.start > ix {
                    path.segs.start -= 1;
                }
                if path.segs.end > ix {
                    path.segs.end -= 1;
                }
            }
        }
    }
}

// A box around some points, while it is being worked out.
struct SmallBox {
    x0: f64,
    x1: f64,
    y0: f64,
    y1: f64,
}

impl SmallBox {
    #[inline(always)]
    fn new() -> Self {
        SmallBox {
            x0: f64::INFINITY,
            x1: f64::NEG_INFINITY,
            y0: f64::INFINITY,
            y1: f64::NEG_INFINITY,
        }
    }

    #[inline(always)]
    fn add(&mut self, p: Point) {
        self.x0 = if p.x < self.x0 { p.x } else { self.x0 };
        self.x1 = if p.x > self.x1 { p.x } else { self.x1 };
        self.y0 = if p.y < self.y0 { p.y } else { self.y0 };
        self.y1 = if p.y > self.y1 { p.y } else { self.y1 };
    }

    #[inline(always)]
    fn get(&self) -> PointBox {
        [self.x0, self.x1, self.y0, self.y1]
    }
}

// The start, middle and end of a segment: eval() at 0, 0.5 and 1.
#[inline(always)]
fn sample(seg: &PathSeg) -> [Point; 3] {
    match seg {
        PathSeg::Line(l) => [l.eval(0.0), l.eval(0.5), l.eval(1.0)],
        // The first and the last control point of a quadratic are what
        // QuadBez::eval() gives at 0 and at 1: there it adds the other
        // points times zero to that one times one.
        PathSeg::Quad(q) => [q.p0, q.eval(0.5), q.p2],
        PathSeg::Cubic(c) => [c.eval(0.0), c.eval(0.5), c.eval(1.0)],
    }
}

// What we keep about a pair of paths, one from each side.
#[derive(Clone, Copy, Default)]
struct PathPair {
    // The closest segments in the last round the pair was measured in.
    closest: Option<(usize, usize)>,
    // The square of a distance the paths are at least apart in this
    // round, whatever way they are measured.
    apart: f64,
}

// The rest of the code is just a terribly inefficient and inaccurate
// way to work out how far apart two sets of paths are at their closest
// point. A real algorithm should use linesweep.
//
// The distance is the smallest of the distances between a left and a
// right path. It used to measure every such pair of paths. It need
// not: a pair whose boxes are farther apart than a pair that has been
// measured already cannot be the smallest, so we measure first the
// pair whose boxes are nearest and then only the pairs that could
// still beat the smallest so far.
fn path_distance(
    left: &Segments,
    right: &Segments,
    pairs: &mut [PathPair],
    near: &mut Vec<(PointBox, usize)>,
) -> Option<f32> {
    // The pair whose boxes are nearest to each other.
    let mut nearest = 0;
    let mut least = f64::INFINITY;
    let mut all = pairs.iter_mut().enumerate();
    for path1 in &left.paths {
        for path2 in &right.paths {
            if let Some((k, pair)) = all.next() {
                pair.apart = if left.cubics || right.cubics {
                    // Cubics are not measured at all, see below.
                    f64::NEG_INFINITY
                } else {
                    at_least_apart(&path1.around, &path2.around)
                };
                if pair.apart < least {
                    least = pair.apart;
                    nearest = k;
                }
            }
        }
    }

    let mut min_distance: Option<f64> = None;
    let width = right.paths.len();
    let first = [nearest];
    for k in first.into_iter().chain(0..pairs.len()) {
        let (Some(pair), Some(p1), Some(p2)) = (
            pairs.get_mut(k),
            left.paths.get(k / width.max(1)),
            right.paths.get(k % width.max(1)),
        ) else {
            continue;
        };
        // The square of the smallest distance measured so far, and a
        // bit: what is at least that far apart cannot be nearer.
        let mut limit = f64::INFINITY;
        if let Some(min) = min_distance {
            limit = min * min * (1.0 + 1e-9);
            // This pair is farther apart than one we have measured, or
            // it is that one.
            if pair.apart > limit || k == nearest {
                continue;
            }
        }
        let d = min_distance_bezpath(left, p1, right, p2, &mut pair.closest, near, limit);
        // log::debug!("  d={:?}", d);
        if d.is_nan() {
            // Two curves that touch can measure as "not a number".
            // Going through the pairs in order, that is what the whole
            // result was if it happened to the very first pair, and
            // something to ignore for any other pair.
            if k == 0 {
                return Some(d as f32);
            }
        } else if min_distance.is_none() || d < min_distance.unwrap() {
            // log::debug!("    (new record)");
            min_distance = Some(d)
        } else {
            // log::debug!("    (ignored)");
        }
    }
    min_distance.map(|x| x as f32)
}

// The square of a distance that two paths are certainly measured to be
// at least apart, given the boxes around their control points.
//
// Each of the ways of measuring below gives the distance between a
// point of one segment and a point of another, or rather its square,
// worked out in floating point. Points of a segment are inside the box
// around its control points, so the true value is at least the square
// of the gap between the boxes. The rounding errors of the worked out
// value can be bounded in terms of the largest coordinate M: they are
// below 2^-44 M^2 for the Bernstein form that kurbo's min_dist() uses
// (it subtracts squares of coordinates, which is where most of it
// comes from) and smaller for the others. We allow 2^-36 M^2.
#[inline(always)]
fn at_least_apart(a: &PointBox, b: &PointBox) -> f64 {
    let mut largest = 0.0;
    for &coordinate in a.iter().chain(b.iter()) {
        if coordinate.abs() > largest {
            largest = coordinate.abs();
        }
    }
    box_gap_squared(a, b) * (1.0 - 1e-9) - rounding(largest)
}

// The allowance for rounding when points are measured whose coordinates
// are no larger than `largest`: 2^-36 M^2.
#[inline(always)]
fn rounding(largest: f64) -> f64 {
    const ROUNDING: f64 = 1.0 / (1u64 << 36) as f64;
    ROUNDING * largest * largest
}

// Work out which are the closest pair of segments, and how far apart
// those two are. `hint` is the pair it was the last time, if these paths
// have been compared before, and becomes the pair it is now.
fn min_distance_bezpath(
    one: &Segments,
    path1: &Path,
    other: &Segments,
    path2: &Path,
    hint: &mut Option<(usize, usize)>,
    near: &mut Vec<(PointBox, usize)>,
    limit: f64,
) -> f64 {
    *hint = closest_segments(one, path1, other, path2, *hint, near);
    if let Some((i1, i2)) = *hint {
        let (s1, s2) = (one.segs[i1], other.segs[i2]);
        // debug(&format!("Best pair was {:?}, {:?}", s1, s2));
        match (s1, s2) {
            (PathSeg::Line(l1), PathSeg::Line(l2)) => line_line_dist(l1, l2),
            (PathSeg::Line(l1), PathSeg::Quad(c2)) => line_curve_dist(l1, c2, limit),
            (PathSeg::Quad(c1), PathSeg::Line(l2)) => line_curve_dist(l2, c1, limit),
            // What s1.min_dist(s2, 0.5).distance is.
            (PathSeg::Quad(c1), PathSeg::Quad(c2)) => quad_quad_dist(&c1, &c2),
            _ => {
                debug("Unusual configuration");
                0.0
            }
        }
    } else {
        f64::MAX
    }
}

// The pair of segments, one of each path, that are closest going by
// their three points: the start, middle and end of one segment are
// compared with the start, middle and end of the other, and the
// smallest of the three distances counts. Of several pairs that are
// equally close, the last one wins: the one with the later segment of
// the first path and, between those, the later segment of the second.
//
// This used to try all combinations, and that is still what it finds,
// but most combinations can be ruled out without measuring them:
//
// - Three points of one segment cannot be nearer to three points of
//   another than the boxes around them are. So when two boxes are
//   farther apart than the best pair so far, none of the segments in
//   them needs looking at; and when a box is that far from the box
//   around all the points of the other path, it need not be compared
//   with any of that path's boxes. We start with a pair that is likely
//   to be close, to have a good "best so far" early.
// - The squares of the distances are compared first, which avoids
//   three calls to hypot() for almost all pairs that are looked at.
//
// Either way a pair is only left out when it is certainly farther apart
// than the best pair, so that it could neither win nor tie.
fn closest_segments(
    one: &Segments,
    path1: &Path,
    other: &Segments,
    path2: &Path,
    hint: Option<(usize, usize)>,
    near: &mut Vec<(PointBox, usize)>,
) -> Option<(usize, usize)> {
    let points1 = &one.points[path1.segs.clone()];
    let points2 = &other.points[path2.segs.clone()];
    let boxes1 = &one.boxes[path1.boxes.clone()];
    let boxes2 = &other.boxes[path2.boxes.clone()];
    let mut closest = Closest {
        distance: f64::INFINITY,
        limit: f64::INFINITY,
        pair: None,
    };

    // Something to start with: the closest pair of the last round, or
    // else the segments in the box of the first path that is nearest
    // to the second path and the box of the second path nearest to it.
    let hint = hint.and_then(|(i1, i2)| {
        let i1 = i1.checked_sub(path1.segs.start)?;
        let i2 = i2.checked_sub(path2.segs.start)?;
        Some((i1, points1.get(i1)?, i2, points2.get(i2)?))
    });
    if let Some((i1, p1, i2, p2)) = hint {
        closest.offer(i1, p1, i2, p2);
    } else if let Some((b1, box1)) = nearest_box(boxes1, &path2.points) {
        if let Some((b2, _)) = nearest_box(boxes2, box1) {
            closest.offer_all(b1, points1, b2, points2);
        }
    }

    // The boxes of the second path that are near enough to the first
    // path to matter, and the box around those.
    near.clear();
    let mut around = SmallBox::new();
    for (b2, box2) in boxes2.iter().enumerate() {
        if box_gap_squared(box2, &path1.points) > closest.limit {
            continue;
        }
        near.push((*box2, b2));
        around.add(Point::new(box2[0], box2[2]));
        around.add(Point::new(box2[1], box2[3]));
    }
    let around = around.get();

    for (b1, box1) in boxes1.iter().enumerate() {
        if box_gap_squared(box1, &around) > closest.limit {
            continue;
        }
        for (box2, b2) in near.iter() {
            if box_gap_squared(box1, box2) > closest.limit {
                continue;
            }
            closest.offer_all(b1, points1, *b2, points2);
        }
    }
    closest
        .pair
        .map(|(i1, i2)| (path1.segs.start + i1, path2.segs.start + i2))
}

// Which of the boxes is nearest to another box.
#[inline(always)]
fn nearest_box<'a>(boxes: &'a [PointBox], to: &PointBox) -> Option<(usize, &'a PointBox)> {
    let mut nearest = None;
    let mut least = f64::INFINITY;
    for (b, around) in boxes.iter().enumerate() {
        let gap = box_gap_squared(around, to);
        if nearest.is_none() || gap < least {
            least = gap;
            nearest = Some((b, around));
        }
    }
    nearest
}

// The square of the distance between two boxes: no point of one box is
// nearer than this to a point of the other, in x and in y separately,
// and so also when the differences are rounded, squared and added up
// the way Closest::offer() does it.
#[inline(always)]
fn box_gap_squared(a: &PointBox, b: &PointBox) -> f64 {
    let dx = positive(a[0] - b[1], b[0] - a[1]);
    let dy = positive(a[2] - b[3], b[2] - a[3]);
    dx * dx + dy * dy
}

// Whichever of the two is positive (they never both are), or zero.
#[inline(always)]
fn positive(a: f64, b: f64) -> f64 {
    if a > 0.0 {
        a
    } else if b > 0.0 {
        b
    } else {
        0.0
    }
}

// The closest pair of segments found so far.
struct Closest {
    // How far apart the pair is.
    distance: f64,
    // Its square and a bit: two points whose squared distance is more
    // than this are farther apart than `distance`.
    limit: f64,
    pair: Option<(usize, usize)>,
}

impl Closest {
    // Look at one pair of segments.
    #[inline(always)]
    fn offer(&mut self, i1: usize, p1: &[Point; 3], i2: usize, p2: &[Point; 3]) {
        let (x0, y0) = (p1[0].x - p2[0].x, p1[0].y - p2[0].y);
        let (x1, y1) = (p1[1].x - p2[1].x, p1[1].y - p2[1].y);
        let (x2, y2) = (p1[2].x - p2[2].x, p1[2].y - p2[2].y);
        let square0 = x0 * x0 + y0 * y0;
        let square1 = x1 * x1 + y1 * y1;
        let square2 = x2 * x2 + y2 * y2;
        let limit = self.limit;
        if !((square0 <= limit) | (square1 <= limit) | (square2 <= limit)) {
            return;
        }
        self.take(i1, i2, [(x0, y0, square0), (x1, y1, square1), (x2, y2, square2)]);
    }

    // The pair of segments has points that are not over the limit:
    // see how far apart it is, and whether it is the closest now. Each
    // of `between` is the differences in x and y between two points and
    // the sum of their squares.
    #[cold]
    #[inline(never)]
    fn take(&mut self, i1: usize, i2: usize, between: [(f64, f64, f64); 3]) {
        let [(x0, y0, square0), (x1, y1, square1), (x2, y2, square2)] = between;
        let limit = self.limit;
        // The distance of the pair is the least of the three distances.
        // Those over the limit are more than `distance` and do not
        // matter, if there are others.
        let mut distance = f64::INFINITY;
        if square0 <= limit {
            distance = x0.hypot(y0);
        }
        if square1 <= limit {
            let d = x1.hypot(y1);
            if d < distance {
                distance = d;
            }
        }
        if square2 <= limit {
            let d = x2.hypot(y2);
            if d < distance {
                distance = d;
            }
        }
        if distance < self.distance {
            self.distance = distance;
            // hypot() and the sum of squares are both correct to within a
            // few units in the last place; this allows for far more.
            self.limit = distance * distance * (1.0 + 1e-9);
            self.pair = Some((i1, i2));
        } else if distance == self.distance && Some((i1, i2)) > self.pair {
            self.pair = Some((i1, i2));
        }
    }

    // Look at all pairs of segments of two boxes.
    #[inline(always)]
    fn offer_all(&mut self, b1: usize, points1: &[[Point; 3]], b2: usize, points2: &[[Point; 3]]) {
        let (start1, start2) = (b1 * BOXED, b2 * BOXED);
        let end1 = (start1 + BOXED).min(points1.len());
        let end2 = (start2 + BOXED).min(points2.len());
        let (points1, points2) = (&points1[start1..end1], &points2[start2..end2]);
        // Full boxes are the usual case: say so, so that the loops are
        // unrolled.
        if let (Ok(points1), Ok(points2)) = (
            <&[[Point; 3]; BOXED]>::try_from(points1),
            <&[[Point; 3]; BOXED]>::try_from(points2),
        ) {
            for (i1, p1) in points1.iter().enumerate() {
                for (i2, p2) in points2.iter().enumerate() {
                    self.offer(start1 + i1, p1, start2 + i2, p2);
                }
            }
        } else {
            for (i1, p1) in points1.iter().enumerate() {
                for (i2, p2) in points2.iter().enumerate() {
                    self.offer(start1 + i1, p1, start2 + i2, p2);
                }
            }
        }
    }
}

fn line_line_dist(l1: kurbo::Line, l2: kurbo::Line) -> f64 {
    let a = l1.nearest(l2.p0, 1.0).distance_sq;
    let b = l1.nearest(l2.p1, 1.0).distance_sq;
    let c = l2.nearest(l1.p0, 1.0).distance_sq;
    let d = l2.nearest(l1.p1, 1.0).distance_sq;
    (a.min(b).min(c).min(d)).sqrt()
}

// The distance from the curve to the nearest of six points of the line.
//
// Finding the point of the curve nearest to a point takes solving a
// cubic. It is not done for a point that is farther from the curve than
// the nearest distance found so far, as far as we can tell without
// solving: the search could only come up with something larger.
//
// The square of the distance from a point to the point of the curve at
// t is a polynomial in t of degree four. Its five Bernstein
// coefficients are made of the dot products of the control points, as
// seen from the point, and for t from 0 to 1 it is a weighted mean of
// them: nowhere below the smallest, which is therefore the square of
// how far the point is at least from the curve. The coefficients and
// the distance kurbo finds are both rounded, each by less than
// 2^-44 M^2 for a largest coordinate M, and we allow 2^-36 M^2 as
// at_least_apart() does. The points are tried nearest first, going by
// that.
//
// Nor is it done for a point that is farther from the curve than
// `limit`, the square of a distance that the caller has no use for
// anything above: the result is then just some distance above it.
fn line_curve_dist(l1: kurbo::Line, c1: kurbo::QuadBez, limit: f64) -> f64 {
    let t = [0.0, 0.2, 0.4, 0.6, 0.8, 0.99];
    let points = t.map(|x| l1.eval(x));
    let mut largest = 0.0;
    for coordinate in [
        c1.p0.x, c1.p0.y, c1.p1.x, c1.p1.y, c1.p2.x, c1.p2.y, l1.p0.x, l1.p0.y, l1.p1.x, l1.p1.y,
    ] {
        if coordinate.abs() > largest {
            largest = coordinate.abs();
        }
    }
    let rounding = rounding(largest);
    // The square of how far each point is at least from the curve.
    let mut apart = points.map(|p| {
        let (x0, y0) = (c1.p0.x - p.x, c1.p0.y - p.y);
        let (x1, y1) = (c1.p1.x - p.x, c1.p1.y - p.y);
        let (x2, y2) = (c1.p2.x - p.x, c1.p2.y - p.y);
        let mut least = x0 * x0 + y0 * y0;
        for coefficient in [
            x0 * x1 + y0 * y1,
            (x0 * x2 + y0 * y2 + 2.0 * (x1 * x1 + y1 * y1)) * (1.0 / 3.0),
            x1 * x2 + y1 * y2,
            x2 * x2 + y2 * y2,
        ] {
            if coefficient < least {
                least = coefficient;
            }
        }
        let apart = least * (1.0 - 1e-9) - rounding;
        // Should the numbers be too large to say, say nothing.
        if apart < f64::INFINITY {
            apart
        } else {
            f64::NEG_INFINITY
        }
    });
    // What a point must not be farther than to matter: the limit, or
    // the nearest distance found so far and a bit.
    let mut reach = limit;
    let mut nearest: Option<f64> = None;
    for _ in &points {
        // The nearest of the points that have not been tried: those
        // that have are marked as infinitely far.
        let mut next = 0;
        for (i, &this) in apart.iter().enumerate() {
            if this < apart[next] {
                next = i;
            }
        }
        // The points that are left are all farther than this.
        if apart[next] > reach {
            break;
        }
        let distance_sq = c1.nearest(points[next], 1.0).distance_sq;
        nearest = Some(nearest.map_or(distance_sq, |nearest| nearest.min(distance_sq)));
        if distance_sq * (1.0 + 1e-9) < reach {
            reach = distance_sq * (1.0 + 1e-9);
        }
        apart[next] = f64::INFINITY;
    }
    nearest.unwrap_or(f64::MAX).sqrt()
}
