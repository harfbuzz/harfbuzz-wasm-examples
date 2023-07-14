use core::cmp::Ordering;
use harfbuzz_wasm::debug;
use kurbo::{BezPath, ParamCurve, ParamCurveNearest, PathSeg};

pub fn _determine_kern(
    left_paths: &[BezPath],
    right_paths: &[BezPath],
    target_distance: f32,
    scale_factor: f32,
) -> f32 {
    let right_paths: Vec<BezPath> = right_paths.clone().into();
    // debug(&format!("Left paths were {:?}", left_paths));
    // debug(&format!("Right paths were {:?}", right_paths));

    let minimum_possible = -1000.0 * scale_factor;
    let mut kern = 0.0;

    if let Some(md) = path_distance(left_paths, &right_paths) {
        let min_distance = md;
        // debug(&format!(
        //     "Default distance between paths is {}",
        //     min_distance,
        // ));
        let this_kern = target_distance - min_distance;
        kern += this_kern;
        // debug(&format!("Kern applied is {}", kern));
        if kern < minimum_possible {
            return minimum_possible;
        }
    } else {
        return minimum_possible;
    }
    kern
}

pub fn path_distance(left_paths: &[BezPath], right_paths: &[BezPath]) -> Option<f32> {
    let mut min_distance: Option<f64> = None;
    for p1 in left_paths {
        for p2 in right_paths {
            let d = min_distance_bezpath(p1, p2);
            // debug(&format!("  d={:?}", d));
            if min_distance.is_none() || d < min_distance.unwrap() {
                // debug(&"    (new record)".to_string());
                min_distance = Some(d)
            } else {
                // debug(&"    (ignored)".to_string());
            }
        }
    }
    min_distance.map(|x| x as f32)
}

fn min_distance_bezpath(one: &BezPath, other: &BezPath) -> f64 {
    let segs1 = one.segments();
    let mut best_pair: Option<(f64, kurbo::PathSeg, kurbo::PathSeg)> = None;
    for s1 in segs1 {
        let p1 = vec![s1.eval(0.0), s1.eval(0.5), s1.eval(1.0)];
        for s2 in other.segments() {
            let p2 = vec![s2.eval(0.0), s2.eval(0.5), s2.eval(1.0)];
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
            best_pair = Some((dist, s1, s2));
        }
    }
    if let Some((_, s1, s2)) = best_pair {
        // debug(&format!("Best pair was {:?}, {:?}", s1, s2));
        match (s1, s2) {
            (PathSeg::Line(l1), PathSeg::Line(l2)) => line_line_dist(l1, l2),
            (PathSeg::Line(l1), PathSeg::Quad(c2)) => line_curve_dist(l1, c2),
            (PathSeg::Line(l1), PathSeg::Cubic(c2)) => line_curve_dist_c(l1, c2),
            (PathSeg::Quad(c1), PathSeg::Line(l2)) => line_curve_dist(l2, c1),
            (PathSeg::Cubic(c1), PathSeg::Line(l2)) => line_curve_dist_c(l2, c1),
            (PathSeg::Quad(_c1), PathSeg::Quad(_c2)) => s1.min_dist(s2, 0.5).distance,
            (PathSeg::Cubic(_c1), PathSeg::Cubic(_c2)) => s1.min_dist(s2, 0.5).distance,
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
    let t = [0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9];
    t.iter()
        .map(|x| c1.nearest(l1.eval(*x), 1.0).distance_sq)
        .reduce(|a, b| a.min(b))
        .unwrap_or(f64::MAX)
        .sqrt()
}

fn line_curve_dist_c(l1: kurbo::Line, c1: kurbo::CubicBez) -> f64 {
    let t = [0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9];
    t.iter()
        .map(|x| c1.nearest(l1.eval(*x), 1.0).distance_sq)
        .reduce(|a, b| a.min(b))
        .unwrap_or(f64::MAX)
        .sqrt()
}
