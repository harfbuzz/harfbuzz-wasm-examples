// The distance between two quadratic Béziers that kurbo's
// PathSeg::min_dist() gives at an accuracy of 0.5: the very same
// number, with far less work.
//
// kurbo (0.9.5, mindist.rs) follows "Computing the minimum distance
// between two Bézier curves", Chen et al., Journal of Computational
// and Applied Mathematics 229 (2009). The squared distance between a
// point of one curve and a point of the other is a polynomial
//     S(u, v) = sum over r, k of D_rk * B_r(u) * B_k(v)
// and kurbo walks a tree of rectangles of (u, v), starting with the
// unit square: it works S out at the four corners, and compares the
// coefficients D_rk with the smallest of the four to decide whether to
// stop there or to cut the rectangle into four. It never cuts the
// curves themselves, so D_rk is the same in every rectangle, and yet
// it works every D_rk out again from the control points each time one
// is used: 1,125 times on average for a pair of quadratics, for 25
// different values, with the binomials and the powers of u and v
// redone each time.
//
// Here the coefficients are worked out once per call, the powers once
// per value of u or v, and S once per corner (the four pieces of a
// rectangle share five of their corners with each other and four with
// the rectangle). Every floating-point operation that a result depends
// on is the one kurbo does, on the same operands, in the same order,
// so the result has the same bits. Three facts are used to leave
// operations out. They hold as long as no intermediate value
// overflows or is not a number to begin with, which cannot happen
// here: dist.rs only comes here with finite coordinates, and those
// start as 32-bit floats.
//
// - x * 1.0 and x + 0.0 are x. (But for the sign of a zero, which can
//   end up in a coefficient. It changes no comparison, and it does not
//   reach a value of S: that is a sum which starts at +0.0, and such a
//   sum is never a negative zero.)
// - What kurbo's tests on the coefficients decide is the same in every
//   rectangle, because the coefficients are. So the smallest
//   coefficient, where it is, and the four boundary flags are found
//   once; and the boundary test can only stop the walk at the unit
//   square, since nothing is below it unless the test failed there.
// - A term with a basis value of zero (u or v at 0 or 1) adds zero to
//   a sum, and leaves it as it was. So where u is 0 or 1 only one row
//   of coefficients is summed, and where v is, one column.

use kurbo::QuadBez;

const EPSILON: f64 = 0.5;

// A quadratic's control points: x0, y0, x1, y1, x2, y2.
type Quad = [f64; 6];

// kurbo's A_r(r, p) for r = 0..=4 and three control points:
// $ A_r=\sum _{i=\theta} ^\upsilon (P_i \cdot P_{r-i}) C_n^i C_n^{r-i} / C_{2n}^r $
// The factors are 1, 1/2, 1/6 and 4/6, and the terms are added up in
// the order of i.
fn a_coefficients(p: &Quad) -> [f64; 5] {
    const F16: f64 = 1.0 / 6.0;
    const F46: f64 = 4.0 / 6.0;
    let d00 = p[0] * p[0] + p[1] * p[1];
    let d01 = p[0] * p[2] + p[1] * p[3];
    let d02 = p[0] * p[4] + p[1] * p[5];
    let d11 = p[2] * p[2] + p[3] * p[3];
    let d12 = p[2] * p[4] + p[3] * p[5];
    let d22 = p[4] * p[4] + p[5] * p[5];
    [
        d00,
        d01 * 0.5 + d01 * 0.5,
        d02 * F16 + d11 * F46 + d02 * F16,
        d12 * 0.5 + d12 * 0.5,
        d22,
    ]
}

// One of the two factors of kurbo's C_rk(r, k, ..), for r = 0..=4:
// $ \sum_{i=\theta}^\upsilon P_i C_n^i C_n^{r-i} / C_{2n}^r $
// kurbo multiplies each point by C(2, i) C(2, r - i) and then by the
// reciprocal of C(4, r) (that is how its Vec2 divides), and adds the
// terms up in the order of i. Returns the x's and the y's.
fn c_factors(p: &Quad) -> ([f64; 5], [f64; 5]) {
    const R4: f64 = 1.0 / 4.0;
    const R6: f64 = 1.0 / 6.0;
    let f = |c0: f64, c1: f64, c2: f64| -> [f64; 5] {
        [
            c0,
            (c0 * 2.0) * R4 + (c1 * 2.0) * R4,
            c0 * R6 + (c1 * 4.0) * R6 + c2 * R6,
            (c1 * 2.0) * R4 + (c2 * 2.0) * R4,
            c2,
        ]
    };
    (f(p[0], p[2], p[4]), f(p[1], p[3], p[5]))
}

// kurbo's basis_function(4, i, u) for i = 0..=4, which is
// C(4, i) * (1 - u).powi(4 - i) * u.powi(i); and, if u is 0 or 1, which
// of the five is 1 (the others are 0 then).
struct Basis {
    b: [f64; 5],
    end: Option<usize>,
}

const BASIS_AT_0: Basis = Basis {
    b: [1.0, 0.0, 0.0, 0.0, 0.0],
    end: Some(0),
};
const BASIS_AT_1: Basis = Basis {
    b: [0.0, 0.0, 0.0, 0.0, 1.0],
    end: Some(4),
};

// For u between 0 and 1. The compiled kurbo gets the powers from
// compiler-builtins' __powidf2(), which multiplies like this:
// x^2 = x * x, x^3 = x * (x * x), x^4 = (x * x) * (x * x). They are
// written out, so that the multiplications are these whatever a
// compiler makes of powi() with an exponent it knows.
fn basis(u: f64) -> Basis {
    let m = 1.0 - u;
    let m2 = m * m;
    let m3 = m * m2;
    let m4 = m2 * m2;
    let u2 = u * u;
    let u3 = u * u2;
    let u4 = u2 * u2;
    Basis {
        b: [m4, (4.0 * m3) * u, (6.0 * m2) * u2, (4.0 * m) * u3, u4],
        end: None,
    }
}

// What f64::min() gives for two numbers.
#[inline(always)]
fn least(a: f64, b: f64) -> f64 {
    if b < a {
        b
    } else {
        a
    }
}

struct Surface {
    // D_rk for r and k from 0 to 4.
    d: [[f64; 5]; 5],
    // The smallest D_rk with r, k < 4: kurbo's "property one" stops at
    // a rectangle when none of those is below the smallest corner.
    low: f64,
    // Where kurbo cuts a rectangle, as a fraction of its width: at
    // r / 4 and k / 4 for the first D_rk that is the smallest.
    split_u: f64,
    split_v: f64,
}

impl Surface {
    // kurbo's S(u, v), from the basis values of u and of v.
    fn eval(&self, bu: &Basis, bv: &Basis) -> f64 {
        let mut sum = 0.0;
        match (bu.end, bv.end) {
            (Some(r), Some(k)) => sum += self.d[r][k],
            (Some(r), None) => {
                for k in 0..5 {
                    sum += self.d[r][k] * bv.b[k];
                }
            }
            (None, Some(k)) => {
                for r in 0..5 {
                    sum += self.d[r][k] * bu.b[r];
                }
            }
            (None, None) => {
                for r in 0..5 {
                    for k in 0..5 {
                        sum += self.d[r][k] * bu.b[r] * bv.b[k];
                    }
                }
            }
        }
        sum
    }

    // What kurbo's min_dist_param() returns for a rectangle inside the
    // unit square (the first of its three numbers). `s` are the values
    // at the corners, in kurbo's order: (umin, vmin), (umin, vmax),
    // (umax, vmin), (umax, vmax).
    #[inline(always)]
    fn piece(
        &self,
        u: (f64, f64),
        bu: (&Basis, &Basis),
        v: (f64, f64),
        bv: (&Basis, &Basis),
        s: [f64; 4],
        best_alpha: f64,
    ) -> f64 {
        let alpha = least(least(least(s[0], s[1]), s[2]), s[3]);
        if alpha > best_alpha
            || (u.1 - u.0).abs() < EPSILON
            || (v.1 - v.0).abs() < EPSILON
            || !(self.low < alpha)
        {
            return alpha;
        }
        self.cut(u, bu, v, bv, s, alpha)
    }

    // The rest of min_dist_param() for a rectangle it does not stop at:
    // cut it into four and take the least of what the four give.
    fn cut(
        &self,
        u: (f64, f64),
        bu: (&Basis, &Basis),
        v: (f64, f64),
        bv: (&Basis, &Basis),
        s: [f64; 4],
        alpha: f64,
    ) -> f64 {
        // A rectangle that is cut at the very start of its range has
        // that start twice: the same parameter value, so the same basis
        // values and the same values of S.
        let same_u = self.split_u == 0.0;
        let same_v = self.split_v == 0.0;
        let nu = u.0 + (u.1 - u.0) * self.split_u;
        let nv = v.0 + (v.1 - v.0) * self.split_v;
        let basis_nu;
        let bnu = if same_u {
            bu.0
        } else {
            basis_nu = basis(nu);
            &basis_nu
        };
        let basis_nv;
        let bnv = if same_v {
            bv.0
        } else {
            basis_nv = basis(nv);
            &basis_nv
        };
        let s_u0_nv = if same_v { s[0] } else { self.eval(bu.0, bnv) };
        let s_u1_nv = if same_v { s[2] } else { self.eval(bu.1, bnv) };
        let s_nu_v0 = if same_u { s[0] } else { self.eval(bnu, bv.0) };
        let s_nu_v1 = if same_u { s[1] } else { self.eval(bnu, bv.1) };
        let s_nu_nv = if same_u {
            s_u0_nv
        } else if same_v {
            s_nu_v0
        } else {
            self.eval(bnu, bnv)
        };
        let results = [
            self.piece(
                (u.0, nu),
                (bu.0, bnu),
                (v.0, nv),
                (bv.0, bnv),
                [s[0], s_u0_nv, s_nu_v0, s_nu_nv],
                alpha,
            ),
            self.piece(
                (u.0, nu),
                (bu.0, bnu),
                (nv, v.1),
                (bnv, bv.1),
                [s_u0_nv, s[1], s_nu_nv, s_nu_v1],
                alpha,
            ),
            self.piece(
                (nu, u.1),
                (bnu, bu.1),
                (v.0, nv),
                (bv.0, bnv),
                [s_nu_v0, s_nu_nv, s[2], s_u1_nv],
                alpha,
            ),
            self.piece(
                (nu, u.1),
                (bnu, bu.1),
                (nv, v.1),
                (bnv, bv.1),
                [s_nu_nv, s_nu_v1, s_u1_nv, s[3]],
                alpha,
            ),
        ];
        // kurbo: min_by(partial_cmp, or Less), which keeps the earlier
        // of two it cannot tell apart.
        let mut least = results[0];
        for &r in &results[1..] {
            if least > r {
                least = r;
            }
        }
        least
    }
}

// What PathSeg::Quad(q1).min_dist(PathSeg::Quad(q2), 0.5).distance is.
pub fn quad_quad_dist(q1: &QuadBez, q2: &QuadBez) -> f64 {
    let p: Quad = [q1.p0.x, q1.p0.y, q1.p1.x, q1.p1.y, q1.p2.x, q1.p2.y];
    let q: Quad = [q2.p0.x, q2.p0.y, q2.p1.x, q2.p1.y, q2.p2.x, q2.p2.y];
    // D_rk = A_r + B_k - 2 C_rk, where B is A for the other curve and
    // C_rk the dot product of one factor for each curve.
    let a = a_coefficients(&p);
    let b = a_coefficients(&q);
    let (lx, ly) = c_factors(&p);
    let (rx, ry) = c_factors(&q);
    let mut d = [[0.0; 5]; 5];
    for r in 0..5 {
        for k in 0..5 {
            d[r][k] = a[r] + b[k] - 2.0 * (lx[r] * rx[k] + ly[r] * ry[k]);
        }
    }

    // The corners of the unit square, (0, 0), (0, 1), (1, 0), (1, 1):
    // the basis values there are 1 for one index and 0 for the others.
    let s = [0.0 + d[0][0], 0.0 + d[0][4], 0.0 + d[4][0], 0.0 + d[4][4]];
    let alpha = least(least(least(s[0], s[1]), s[2]), s[3]);

    // Property one: D(r>k) > alpha. kurbo cuts at the first of the
    // smallest coefficients.
    let mut low = d[0][0];
    let mut low_at = (0, 0);
    for r in 0..4 {
        for k in 0..4 {
            if d[r][k] < low {
                low = d[r][k];
                low_at = (r, k);
            }
        }
    }
    if !(low < alpha) {
        return alpha.sqrt();
    }

    // Property two: boundary check
    let mut at_boundary0on_bez1 = true;
    let mut at_boundary1on_bez1 = true;
    let mut at_boundary0on_bez2 = true;
    let mut at_boundary1on_bez2 = true;
    for i in 0..4 {
        for j in 0..4 {
            let dij = d[i][j];
            if dij < d[0][j] {
                at_boundary0on_bez1 = false;
            }
            if dij < d[4][j] {
                at_boundary1on_bez1 = false;
            }
            if dij < d[i][0] {
                at_boundary0on_bez2 = false;
            }
            if dij < d[i][4] {
                at_boundary1on_bez2 = false;
            }
        }
    }
    if at_boundary0on_bez1 && at_boundary0on_bez2 {
        return s[0].sqrt();
    }
    if at_boundary0on_bez1 && at_boundary1on_bez2 {
        return s[1].sqrt();
    }
    if at_boundary1on_bez1 && at_boundary0on_bez2 {
        return s[2].sqrt();
    }
    if at_boundary1on_bez1 && at_boundary1on_bez2 {
        return s[3].sqrt();
    }

    let surface = Surface {
        d,
        low,
        split_u: low_at.0 as f64 / 4.0,
        split_v: low_at.1 as f64 / 4.0,
    };
    // kurbo does not stop at the unit square (its tests are the ones
    // above, and a width of 1 is not below the accuracy): cut it.
    surface
        .cut(
            (0.0, 1.0),
            (&BASIS_AT_0, &BASIS_AT_1),
            (0.0, 1.0),
            (&BASIS_AT_0, &BASIS_AT_1),
            s,
            alpha,
        )
        .sqrt()
}
