//! Small, dependency-free numerical helpers for the normal distribution.

use std::f64::consts::{PI, SQRT_2};

/// Complementary error function (Numerical Recipes `erfcc`, Chebyshev fit).
/// Fractional error below 1.2e-7 everywhere, including the far tails, which
/// is what conditional survival quantiles need.
pub fn erfc(x: f64) -> f64 {
    let z = x.abs();
    let t = 1.0 / (1.0 + 0.5 * z);
    let ans = t
        * (-z * z - 1.265_512_23
            + t * (1.000_023_68
                + t * (0.374_091_96
                    + t * (0.096_784_18
                        + t * (-0.186_288_06
                            + t * (0.278_868_07
                                + t * (-1.135_203_98
                                    + t * (1.488_515_87
                                        + t * (-0.822_152_23 + t * 0.170_872_77)))))))))
            .exp();
    if x >= 0.0 {
        ans
    } else {
        2.0 - ans
    }
}

/// Standard normal CDF.
pub fn norm_cdf(z: f64) -> f64 {
    0.5 * erfc(-z / SQRT_2)
}

/// Standard normal survival function, accurate for large positive `z`.
pub fn norm_sf(z: f64) -> f64 {
    0.5 * erfc(z / SQRT_2)
}

pub fn norm_pdf(z: f64) -> f64 {
    (-0.5 * z * z).exp() / (2.0 * PI).sqrt()
}

/// Inverse standard normal CDF (Acklam's rational approximation plus one
/// Halley refinement step). Accurate to roughly 1e-9 for p in (1e-300, 1).
pub fn norm_ppf(p: f64) -> f64 {
    if p <= 0.0 {
        return f64::NEG_INFINITY;
    }
    if p >= 1.0 {
        return f64::INFINITY;
    }
    const A: [f64; 6] = [
        -3.969_683_028_665_376e1,
        2.209_460_984_245_205e2,
        -2.759_285_104_469_687e2,
        1.383_577_518_672_69e2,
        -3.066_479_806_614_716e1,
        2.506_628_277_459_239,
    ];
    const B: [f64; 5] = [
        -5.447_609_879_822_406e1,
        1.615_858_368_580_409e2,
        -1.556_989_798_598_866e2,
        6.680_131_188_771_972e1,
        -1.328_068_155_288_572e1,
    ];
    const C: [f64; 6] = [
        -7.784_894_002_430_293e-3,
        -3.223_964_580_411_365e-1,
        -2.400_758_277_161_838,
        -2.549_732_539_343_734,
        4.374_664_141_464_968,
        2.938_163_982_698_783,
    ];
    const D: [f64; 4] = [
        7.784_695_709_041_462e-3,
        3.224_671_290_700_398e-1,
        2.445_134_137_142_996,
        3.754_408_661_907_416,
    ];
    const P_LOW: f64 = 0.024_25;
    let x = if p < P_LOW {
        let q = (-2.0 * p.ln()).sqrt();
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    } else if p <= 1.0 - P_LOW {
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q
            / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    } else {
        let q = (-2.0 * (1.0 - p).ln()).sqrt();
        -(((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    };
    // One Halley step against the high-accuracy CDF.
    let e = norm_cdf(x) - p;
    let u = e * (2.0 * PI).sqrt() * (x * x / 2.0).exp();
    let refined = x - u / (1.0 + x * u / 2.0);
    if refined.is_finite() {
        refined
    } else {
        x
    }
}

/// Inverse survival function: z such that P(Z > z) = s. Accurate for tiny s.
pub fn norm_isf(s: f64) -> f64 {
    -norm_ppf(s)
}

/// Median of a slice (not weighted). Returns `None` for empty input.
pub fn median(values: &mut [f64]) -> Option<f64> {
    quantile(values, 0.5)
}

/// Linear-interpolated quantile (type 7). Sorts the slice in place.
pub fn quantile(values: &mut [f64], q: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let pos = q.clamp(0.0, 1.0) * (values.len() - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    let frac = pos - lo as f64;
    Some(values[lo] * (1.0 - frac) + values[hi] * frac)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cdf_known_values() {
        assert!((norm_cdf(0.0) - 0.5).abs() < 1e-7);
        assert!((norm_cdf(1.959_963_985) - 0.975).abs() < 1e-6);
        assert!((norm_sf(1.281_551_566) - 0.1).abs() < 1e-6);
        // Far tail stays positive and accurate in relative terms.
        let s = norm_sf(8.0);
        assert!((s / 6.220_960_574e-16 - 1.0).abs() < 1e-5, "sf(8) = {s}");
    }

    #[test]
    fn ppf_inverts_cdf() {
        for p in [
            1e-12,
            1e-6,
            0.001,
            0.1,
            0.3,
            0.5,
            0.7,
            0.9,
            0.999,
            1.0 - 1e-9,
        ] {
            let z = norm_ppf(p);
            assert!(
                (norm_cdf(z) - p).abs() / p.min(1.0 - p).max(1e-12) < 1e-4,
                "p={p} z={z}"
            );
        }
        assert!((norm_ppf(0.9) - 1.281_551_566).abs() < 1e-6);
        assert!((norm_isf(1e-10) - 6.361_340_902).abs() < 1e-4);
    }

    #[test]
    fn quantiles() {
        let mut v = vec![5.0, 1.0, 3.0, 2.0, 4.0];
        assert_eq!(median(&mut v), Some(3.0));
        assert_eq!(quantile(&mut v, 0.25), Some(2.0));
        assert_eq!(median(&mut []), None);
    }
}
