//! The little linear algebra the fits need: symmetric 3×3 eigenvectors, small dense
//! solves, and Levenberg–Marquardt.

use obj2cad_acis::V3;

pub type M3 = [[f64; 3]; 3];

/// Eigenvalues (ascending) and unit eigenvectors of a symmetric 3×3 matrix (Jacobi).
pub fn eigen_sym(m: M3) -> ([f64; 3], [V3; 3]) {
    let mut a = m;
    let mut v: M3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let scale = m.iter().flatten().fold(0.0f64, |s, x| s.max(x.abs()));
    for _ in 0..32 {
        let off = a[0][1].abs() + a[0][2].abs() + a[1][2].abs();
        if off <= 1e-18 * scale || off == 0.0 {
            break;
        }
        for (p, q) in [(0, 1), (0, 2), (1, 2)] {
            if a[p][q] == 0.0 {
                continue;
            }
            let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
            let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
            let t = if theta == 0.0 { 1.0 } else { t };
            let c = 1.0 / (t * t + 1.0).sqrt();
            let s = t * c;
            // A' = Jᵀ A J
            for row in a.iter_mut() {
                let (akp, akq) = (row[p], row[q]);
                row[p] = c * akp - s * akq;
                row[q] = s * akp + c * akq;
            }
            let (rp, rq) = (a[p], a[q]);
            for k in 0..3 {
                a[p][k] = c * rp[k] - s * rq[k];
                a[q][k] = s * rp[k] + c * rq[k];
            }
            for row in v.iter_mut() {
                let (vkp, vkq) = (row[p], row[q]);
                row[p] = c * vkp - s * vkq;
                row[q] = s * vkp + c * vkq;
            }
        }
    }
    let mut idx = [0usize, 1, 2];
    idx.sort_by(|&i, &j| a[i][i].total_cmp(&a[j][j]));
    let vals = idx.map(|i| a[i][i]);
    let vecs = idx.map(|i| [v[0][i], v[1][i], v[2][i]]);
    (vals, vecs)
}

/// Solve `a x = b` (n×n, row-major) by Gaussian elimination with partial pivoting.
pub fn solve(a: &mut [f64], b: &mut [f64], n: usize) -> Option<Vec<f64>> {
    for col in 0..n {
        let piv =
            (col..n).max_by(|&i, &j| a[i * n + col].abs().total_cmp(&a[j * n + col].abs()))?;
        if a[piv * n + col].abs() < 1e-300 {
            return None;
        }
        if piv != col {
            for k in 0..n {
                a.swap(piv * n + k, col * n + k);
            }
            b.swap(piv, col);
        }
        for row in col + 1..n {
            let f = a[row * n + col] / a[col * n + col];
            if f != 0.0 {
                for k in col..n {
                    a[row * n + k] -= f * a[col * n + k];
                }
                b[row] -= f * b[col];
            }
        }
    }
    let mut x = vec![0.0; n];
    for row in (0..n).rev() {
        let mut s = b[row];
        for k in row + 1..n {
            s -= a[row * n + k] * x[k];
        }
        x[row] = s / a[row * n + row];
    }
    x.iter().all(|v| v.is_finite()).then_some(x)
}

/// Linear least squares: minimize |A x - b|² for `rows` of (a_i, b_i), `N` unknowns.
pub fn least_squares<const N: usize>(
    rows: impl Iterator<Item = ([f64; N], f64)>,
) -> Option<[f64; N]> {
    let mut ata = vec![0.0; N * N];
    let mut atb = vec![0.0; N];
    for (a, b) in rows {
        for i in 0..N {
            atb[i] += a[i] * b;
            for j in 0..N {
                ata[i * N + j] += a[i] * a[j];
            }
        }
    }
    let x = solve(&mut ata, &mut atb, N)?;
    let mut out = [0.0; N];
    out.copy_from_slice(&x);
    Some(out)
}

/// Levenberg–Marquardt on `n` local parameters. `residuals(delta, out)` fills the
/// residuals of the model moved by `delta` (delta = 0 is the current model); `apply`
/// makes a step permanent. Returns the final sum of squares.
/// Stops early once every residual is within `good` (converged enough), when some
/// residual is still beyond `hopeless` after twelve iterations (it won't get there), or when
/// an iteration gains less than a thousandth after the first ten (stalled).
pub fn levenberg_marquardt(
    n: usize,
    iterations: usize,
    mut residuals: impl FnMut(&[f64], &mut Vec<f64>),
    mut apply: impl FnMut(&[f64]),
    step: &[f64],
    good: f64,
    hopeless: f64,
) -> f64 {
    let zero = vec![0.0; n];
    let mut r = Vec::new();
    residuals(&zero, &mut r);
    let m = r.len();
    let mut cost: f64 = r.iter().map(|x| x * x).sum();
    let mut lambda = 1e-3;
    let (mut rp, mut rm) = (Vec::with_capacity(m), Vec::with_capacity(m));
    for it in 0..iterations {
        if r.iter().all(|x| x.abs() <= good) || it >= 12 && r.iter().any(|x| x.abs() > hopeless) {
            break;
        }
        // Jacobian by central differences.
        let mut jac = vec![0.0; m * n];
        for k in 0..n {
            let mut d = zero.clone();
            d[k] = step[k];
            residuals(&d, &mut rp);
            d[k] = -step[k];
            residuals(&d, &mut rm);
            for i in 0..m {
                jac[i * n + k] = (rp[i] - rm[i]) / (2.0 * step[k]);
            }
        }
        let mut jtj = vec![0.0; n * n];
        let mut jtr = vec![0.0; n];
        for i in 0..m {
            let row = &jac[i * n..(i + 1) * n];
            for a in 0..n {
                jtr[a] += row[a] * r[i];
                for b in 0..n {
                    jtj[a * n + b] += row[a] * row[b];
                }
            }
        }
        let mut improved = false;
        for _ in 0..12 {
            let mut a = jtj.clone();
            for k in 0..n {
                a[k * n + k] += lambda * (jtj[k * n + k] + 1e-12);
            }
            let mut b: Vec<f64> = jtr.iter().map(|x| -x).collect();
            let Some(delta) = solve(&mut a, &mut b, n) else {
                lambda *= 10.0;
                continue;
            };
            let mut rn = Vec::new();
            residuals(&delta, &mut rn);
            let new_cost: f64 = rn.iter().map(|x| x * x).sum();
            if new_cost.is_finite() && new_cost < cost {
                apply(&delta);
                let gain = cost - new_cost;
                cost = new_cost;
                r = rn;
                lambda = (lambda / 3.0).max(1e-12);
                improved = gain > cost * if it < 10 { 1e-12 } else { 1e-3 };
                break;
            }
            lambda *= 4.0;
        }
        if !improved {
            break;
        }
    }
    cost
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eigen_of_a_diagonalizable_matrix() {
        let (vals, vecs) = eigen_sym([[2.0, 1.0, 0.0], [1.0, 2.0, 0.0], [0.0, 0.0, 5.0]]);
        assert!(
            (vals[0] - 1.0).abs() < 1e-12
                && (vals[1] - 3.0).abs() < 1e-12
                && (vals[2] - 5.0).abs() < 1e-12
        );
        let v = vecs[0];
        assert!((v[0] + v[1]).abs() < 1e-12 && v[2].abs() < 1e-12);
    }

    #[test]
    fn lm_fits_a_line() {
        // y = 2x + 1 through exact points.
        let pts: Vec<(f64, f64)> = (0..10)
            .map(|i| (f64::from(i), 2.0 * f64::from(i) + 1.0))
            .collect();
        let mut p = [0.0f64, 0.0];
        let cost = {
            let pc = std::cell::Cell::new(p);
            let c = levenberg_marquardt(
                2,
                50,
                |d, out| {
                    out.clear();
                    let q = pc.get();
                    out.extend(
                        pts.iter()
                            .map(|&(x, y)| (q[0] + d[0]) * x + (q[1] + d[1]) - y),
                    );
                },
                |d| {
                    let mut q = pc.get();
                    q[0] += d[0];
                    q[1] += d[1];
                    pc.set(q);
                },
                &[1e-6, 1e-6],
                0.0,
                f64::INFINITY,
            );
            p = pc.get();
            c
        };
        assert!(cost < 1e-18, "{cost}");
        assert!((p[0] - 2.0).abs() < 1e-9 && (p[1] - 1.0).abs() < 1e-9);
    }
}
