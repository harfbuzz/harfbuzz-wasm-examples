use core::ops::Range;
use harfbuzz_wasm::debug;
use kurbo::{BezPath, CubicBez, Line, ParamCurve, ParamCurveNearest, PathEl, PathSeg, Point, QuadBez};

// This is a Rust port of the Nastaliq kerning algorithm
// detailed at https://simoncozens.github.io/nastaliq-autokerning/
// It's horribly inefficient because I coded it in an excited
// hurry. Given two sets of paths, one for the left hand side
// and one for the right hand side, work out how many units to
// kern so that these paths are `target_distance` units apart.

pub fn determine_kern(
    left_paths: &[BezPath],
    right_paths: &[BezPath],
    target_distance: f32,
    max_tuck: f32,
    scale_factor: f32,
) -> f32 {
    // The left paths stay where they are, so take their segments, and
    // the points we compare segments by, once. We're going to slide the
    // right paths around until they fit, so keep their segments apart.
    let mut left = Segments::new(left_paths);
    left.sample();
    let mut right = Segments::new(right_paths);
    // The closest segments of each pair of paths in the last round.
    let mut closest = vec![None; left.ends.len() * right.ends.len()];

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
        right.sample();
        if let Some(md) = path_distance(&left, &right, &mut closest) {
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

// How many segments of a path share a box.
const BOXED: usize = 4;

// A box around some points: the least x, the greatest x, the least y
// and the greatest y.
type PointBox = [f64; 4];

// The segments of some paths, and the three points on each segment that
// segments are first compared by.
struct Segments {
    // The segments of all the paths, one path after the other.
    segs: Vec<PathSeg>,
    // Where the segments of each path end in `segs`.
    ends: Vec<usize>,
    // The segments that close a subpath. There are hardly ever any.
    closers: Vec<usize>,
    // How far the segments still have to be moved along x.
    pending: Option<f64>,
    // The start, middle and end of each segment.
    points: Vec<[Point; 3]>,
    // A box around the points of every BOXED segments of a path: the
    // boxes of the first path, then those of the second, and so on.
    boxes: Vec<PointBox>,
}

// The segments of one path, and the boxes around their points.
#[derive(Clone)]
struct PathRange {
    segs: Range<usize>,
    boxes: Range<usize>,
}

impl Segments {
    fn new(paths: &[BezPath]) -> Self {
        let mut this = Segments {
            segs: vec![],
            ends: Vec::with_capacity(paths.len()),
            closers: vec![],
            pending: None,
            points: vec![],
            boxes: vec![],
        };
        for path in paths {
            this.add_path(path);
            this.ends.push(this.segs.len());
        }
        this
    }

    // Add the segments of a path, the ones BezPath::segments() gives.
    // We need to know which of them close a subpath, though: such a
    // segment is only there while the subpath's two ends differ, and
    // moving the path can make them the same point.
    fn add_path(&mut self, path: &BezPath) {
        let mut start_last: Option<(Point, Point)> = None;
        for &el in path.elements() {
            let (start, last) = start_last.get_or_insert_with(|| {
                let point = match el {
                    PathEl::MoveTo(p) => p,
                    PathEl::LineTo(p) => p,
                    PathEl::QuadTo(_, p2) => p2,
                    PathEl::CurveTo(_, _, p3) => p3,
                    PathEl::ClosePath => Point::ZERO,
                };
                (point, point)
            });
            let seg = match el {
                PathEl::MoveTo(p) => {
                    *start = p;
                    *last = p;
                    continue;
                }
                PathEl::LineTo(p) => PathSeg::Line(Line::new(core::mem::replace(last, p), p)),
                PathEl::QuadTo(p1, p2) => {
                    PathSeg::Quad(QuadBez::new(core::mem::replace(last, p2), p1, p2))
                }
                PathEl::CurveTo(p1, p2, p3) => {
                    PathSeg::Cubic(CubicBez::new(core::mem::replace(last, p3), p1, p2, p3))
                }
                PathEl::ClosePath => {
                    if *last != *start {
                        self.closers.push(self.segs.len());
                        PathSeg::Line(Line::new(core::mem::replace(last, *start), *start))
                    } else {
                        continue;
                    }
                }
            };
            self.segs.push(seg);
        }
    }

    // The segments and boxes of one path after the other.
    fn paths(&self) -> impl Iterator<Item = PathRange> + Clone + '_ {
        let mut seg_start = 0;
        let mut box_start = 0;
        self.ends.iter().map(move |&seg_end| {
            let box_end = box_start + (seg_end - seg_start + BOXED - 1) / BOXED;
            let range = PathRange {
                segs: seg_start..seg_end,
                boxes: box_start..box_end,
            };
            seg_start = seg_end;
            box_start = box_end;
            range
        })
    }

    // Move the segments along x, the next time they are sampled.
    fn move_by(&mut self, dx: f64) {
        self.pending = Some(dx);
    }

    // Work out the points the segments are compared by, having moved the
    // segments first if they were asked to.
    fn sample(&mut self) {
        if let Some(dx) = self.pending.take() {
            // This is what applying Affine::translate((dx, 0.0)) to the
            // paths gives: x + dx, and y as it was.
            for seg in self.segs.iter_mut() {
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
            }
            self.drop_closed();
        }
        self.points.clear();
        self.points.extend(self.segs.iter().map(|seg| match seg {
            PathSeg::Line(l) => [l.eval(0.0), l.eval(0.5), l.eval(1.0)],
            PathSeg::Quad(q) => [q.eval(0.0), q.eval(0.5), q.eval(1.0)],
            PathSeg::Cubic(c) => [c.eval(0.0), c.eval(0.5), c.eval(1.0)],
        }));
        self.boxes.clear();
        let mut start = 0;
        for &end in &self.ends {
            for points in self.points[start..end].chunks(BOXED) {
                let mut around = [f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY];
                for p in points.iter().flatten() {
                    if p.x < around[0] {
                        around[0] = p.x;
                    }
                    if p.x > around[1] {
                        around[1] = p.x;
                    }
                    if p.y < around[2] {
                        around[2] = p.y;
                    }
                    if p.y > around[3] {
                        around[3] = p.y;
                    }
                }
                self.boxes.push(around);
            }
            start = end;
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
            self.closers.remove(k);
            for closer in self.closers[k..].iter_mut() {
                *closer -= 1;
            }
            for end in self.ends.iter_mut() {
                if *end > ix {
                    *end -= 1;
                }
            }
        }
    }
}

// The rest of the code is just a terribly inefficient and inaccurate
// way to work out how far apart two sets of paths are at their closest
// point. A real algorithm should use linesweep.
fn path_distance(
    left: &Segments,
    right: &Segments,
    closest: &mut [Option<(usize, usize)>],
) -> Option<f32> {
    let mut min_distance: Option<f64> = None;
    let mut closest = closest.iter_mut();
    for p1 in left.paths() {
        for p2 in right.paths() {
            let d = match closest.next() {
                Some(hint) => min_distance_bezpath(left, &p1, right, &p2, hint),
                None => min_distance_bezpath(left, &p1, right, &p2, &mut None),
            };
            // log::debug!("  d={:?}", d);
            if min_distance.is_none() || d < min_distance.unwrap() {
                // log::debug!("    (new record)");
                min_distance = Some(d)
            } else {
                // log::debug!("    (ignored)");
            }
        }
    }
    min_distance.map(|x| x as f32)
}

// Work out which are the closest pair of segments, and how far apart
// those two are. `hint` is the pair it was the last time, if these paths
// have been compared before, and becomes the pair it is now.
fn min_distance_bezpath(
    one: &Segments,
    path1: &PathRange,
    other: &Segments,
    path2: &PathRange,
    hint: &mut Option<(usize, usize)>,
) -> f64 {
    *hint = closest_segments(one, path1, other, path2, *hint);
    if let Some((i1, i2)) = *hint {
        let (s1, s2) = (one.segs[i1], other.segs[i2]);
        // debug(&format!("Best pair was {:?}, {:?}", s1, s2));
        match (s1, s2) {
            (PathSeg::Line(l1), PathSeg::Line(l2)) => line_line_dist(l1, l2),
            (PathSeg::Line(l1), PathSeg::Quad(c2)) => line_curve_dist(l1, c2),
            (PathSeg::Quad(c1), PathSeg::Line(l2)) => line_curve_dist(l2, c1),
            (PathSeg::Quad(_c1), PathSeg::Quad(_c2)) => s1.min_dist(s2, 0.5).distance,
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
//   them needs looking at. We start with a pair that is likely to be
//   close, to have a good "best so far" early.
// - The squares of the distances are compared first, which avoids
//   three calls to hypot() for almost all pairs that are looked at.
//
// Either way a pair is only left out when it is certainly farther apart
// than the best pair, so that it could neither win nor tie.
fn closest_segments(
    one: &Segments,
    path1: &PathRange,
    other: &Segments,
    path2: &PathRange,
    hint: Option<(usize, usize)>,
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
    // else the segments in the two boxes that are closest to each other.
    let hint = hint.and_then(|(i1, i2)| {
        let i1 = i1.checked_sub(path1.segs.start)?;
        let i2 = i2.checked_sub(path2.segs.start)?;
        Some((i1, points1.get(i1)?, i2, points2.get(i2)?))
    });
    if let Some((i1, p1, i2, p2)) = hint {
        closest.offer(i1, p1, i2, p2);
    } else {
        let mut nearest = None;
        let mut least = f64::INFINITY;
        for (b1, box1) in boxes1.iter().enumerate() {
            for (b2, box2) in boxes2.iter().enumerate() {
                let gap = box_gap_squared(box1, box2);
                if nearest.is_none() || gap < least {
                    least = gap;
                    nearest = Some((b1, b2));
                }
            }
        }
        if let Some((b1, b2)) = nearest {
            closest.offer_all(b1, points1, b2, points2);
        }
    }

    for (b1, box1) in boxes1.iter().enumerate() {
        for (b2, box2) in boxes2.iter().enumerate() {
            if box_gap_squared(box1, box2) > closest.limit {
                continue;
            }
            closest.offer_all(b1, points1, b2, points2);
        }
    }
    closest
        .pair
        .map(|(i1, i2)| (path1.segs.start + i1, path2.segs.start + i2))
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
        for (i1, p1) in points1[start1..end1].iter().enumerate() {
            for (i2, p2) in points2[start2..end2].iter().enumerate() {
                self.offer(start1 + i1, p1, start2 + i2, p2);
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

fn line_curve_dist(l1: kurbo::Line, c1: kurbo::QuadBez) -> f64 {
    let t = [0.0, 0.2, 0.4, 0.6, 0.8, 0.99];
    t.iter()
        .map(|x| c1.nearest(l1.eval(*x), 1.0).distance_sq)
        .reduce(|a, b| a.min(b))
        .unwrap_or(f64::MAX)
        .sqrt()
}
