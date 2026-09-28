//! Deterministic synthetic OBJ files for benchmarks (no randomness, no dependencies).

use std::fmt::Write as _;

/// A `n × n` vertex terrain grid written like a typical exporter: 6-decimal coordinates,
/// one object, alternating quads and triangle pairs. `(n-1)² · 1.5` faces on average.
pub fn terrain(n: usize) -> Vec<u8> {
    let mut s = String::with_capacity(n * n * 64);
    s.push_str("# obj2cad synthetic terrain\no Terrain\n");
    for i in 0..n {
        for j in 0..n {
            let (x, y) = (i as f64 * 0.5, j as f64 * 0.5);
            let z = 3.0 * (i as f64 * 0.05).sin() * (j as f64 * 0.07).cos() + 0.001 * i as f64;
            let _ = writeln!(s, "v {x:.6} {y:.6} {z:.6}");
        }
    }
    for i in 0..n - 1 {
        for j in 0..n - 1 {
            let a = i * n + j + 1;
            let (b, c, d) = (a + 1, a + n, a + n + 1);
            if (i + j) % 2 == 0 {
                let _ = writeln!(s, "f {a} {b} {d} {c}");
            } else {
                let _ = writeln!(s, "f {a} {b} {d}\nf {a} {d} {c}");
            }
        }
    }
    s.into_bytes()
}
