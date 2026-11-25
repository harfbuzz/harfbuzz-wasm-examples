use core::cmp::Ordering;
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
        if let Some(md) = path_distance(&left, &right) {
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
}

impl Segments {
    fn new(paths: &[BezPath]) -> Self {
        let mut this = Segments {
            segs: vec![],
            ends: Vec::with_capacity(paths.len()),
            closers: vec![],
            pending: None,
            points: vec![],
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

    // The segments of one path after the other.
    fn paths(&self) -> impl Iterator<Item = Range<usize>> + Clone + '_ {
        let mut start = 0;
        self.ends.iter().map(move |&end| {
            let range = start..end;
            start = end;
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
fn path_distance(left: &Segments, right: &Segments) -> Option<f32> {
    let mut min_distance: Option<f64> = None;
    for p1 in left.paths() {
        for p2 in right.paths() {
            let d = min_distance_bezpath(left, p1.clone(), right, p2);
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

// Work out which are the closest pair of segments. I'm sure there's
// a better way to do this than just trying all combinations.
fn min_distance_bezpath(
    one: &Segments,
    range1: Range<usize>,
    other: &Segments,
    range2: Range<usize>,
) -> f64 {
    let mut best_pair: Option<(f64, usize, usize)> = None;
    for i1 in range1 {
        let p1 = &one.points[i1];
        for i2 in range2.clone() {
            let p2 = &other.points[i2];
            let dist = p1
                .iter()
                .zip(p2.iter())
                .map(|(a, b)| a.distance(*b))
                .min_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Less))
                .unwrap();
            if let Some((best, _, _)) = best_pair {
                if dist > best {
                    continue;
                }
            }
            best_pair = Some((dist, i1, i2));
        }
    }
    if let Some((_, i1, i2)) = best_pair {
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
