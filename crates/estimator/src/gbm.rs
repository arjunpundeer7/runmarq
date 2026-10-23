//! Gradient-boosted regression trees with quantile loss.
//!
//! One ensemble per quantile. Each round fits a shallow tree to the sign of
//! the residuals (the negative gradient of the pinball loss) on histogram
//! bins, then sets each leaf to the weighted quantile of the residuals that
//! fall in it. Missing feature values are encoded as -1 by the caller, below
//! every real value, so trees route them like any other value.

use crate::rng::Rng;

#[derive(Debug, Clone)]
pub struct Params {
    pub rounds: usize,
    pub depth: usize,
    pub learning_rate: f64,
    pub min_leaf_weight: f64,
    pub subsample: f64,
    pub bins: usize,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            rounds: 80,
            depth: 3,
            learning_rate: 0.1,
            min_leaf_weight: 20.0,
            subsample: 0.7,
            bins: 32,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Dataset {
    /// Row-major features, `rows * features` values.
    pub x: Vec<f32>,
    pub y: Vec<f64>,
    pub w: Vec<f64>,
    pub features: usize,
}

impl Dataset {
    pub fn new(features: usize) -> Dataset {
        Dataset {
            features,
            ..Default::default()
        }
    }

    pub fn push(&mut self, x: &[f32], y: f64, w: f64) {
        debug_assert_eq!(x.len(), self.features);
        self.x.extend_from_slice(x);
        self.y.push(y);
        self.w.push(w);
    }

    pub fn rows(&self) -> usize {
        self.y.len()
    }

    pub fn row(&self, i: usize) -> &[f32] {
        &self.x[i * self.features..(i + 1) * self.features]
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Node {
    Split {
        feature: u16,
        threshold: f32,
        bin: u8,
        left: u32,
        right: u32,
    },
    Leaf(f64),
}

#[derive(Debug, Clone, PartialEq)]
struct Tree(Vec<Node>);

impl Tree {
    fn predict(&self, x: &[f32]) -> f64 {
        let mut i = 0usize;
        loop {
            match self.0[i] {
                Node::Leaf(v) => return v,
                Node::Split {
                    feature,
                    threshold,
                    left,
                    right,
                    ..
                } => {
                    i = if x[feature as usize] <= threshold {
                        left as usize
                    } else {
                        right as usize
                    }
                }
            }
        }
    }

    fn predict_binned(&self, bins: &[u8]) -> f64 {
        let mut i = 0usize;
        loop {
            match self.0[i] {
                Node::Leaf(v) => return v,
                Node::Split {
                    feature,
                    bin,
                    left,
                    right,
                    ..
                } => {
                    i = if bins[feature as usize] <= bin {
                        left as usize
                    } else {
                        right as usize
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct QuantileModel {
    pub quantile: f64,
    base: f64,
    learning_rate: f64,
    trees: Vec<Tree>,
}

impl QuantileModel {
    pub fn predict(&self, x: &[f32]) -> f64 {
        self.base + self.learning_rate * self.trees.iter().map(|t| t.predict(x)).sum::<f64>()
    }
}

/// Quantile ensembles for 0.1, 0.5 and 0.9.
#[derive(Debug, Clone, PartialEq)]
pub struct Gbm {
    models: [QuantileModel; 3],
    pub rows: usize,
}

impl Gbm {
    pub fn fit(data: &Dataset, params: &Params, seed: u64) -> Option<Gbm> {
        if data.rows() < 10 || data.features == 0 || data.features > u16::MAX as usize {
            return None;
        }
        let binned = Binned::new(data, params.bins.clamp(2, 255));
        let models = [0.1, 0.5, 0.9].map(|q| fit_quantile(data, &binned, params, q, seed));
        Some(Gbm {
            models,
            rows: data.rows(),
        })
    }

    /// Predicted quantiles, sorted so they never cross.
    pub fn predict(&self, x: &[f32]) -> [f64; 3] {
        let mut out = [0, 1, 2].map(|i| self.models[i].predict(x));
        out.sort_by(f64::total_cmp);
        out
    }
}

struct Binned {
    bins: Vec<u8>,
    thresholds: Vec<Vec<f32>>,
    features: usize,
}

impl Binned {
    fn new(data: &Dataset, max_bins: usize) -> Binned {
        let n = data.rows();
        let f = data.features;
        let stride = (n / 4000).max(1);
        let mut thresholds = Vec::with_capacity(f);
        for j in 0..f {
            let mut vals: Vec<f32> = (0..n).step_by(stride).map(|i| data.x[i * f + j]).collect();
            vals.sort_by(f32::total_cmp);
            vals.dedup();
            let cuts: Vec<f32> = if vals.len() <= max_bins {
                vals.iter()
                    .take(vals.len().saturating_sub(1))
                    .copied()
                    .collect()
            } else {
                let mut c: Vec<f32> = (1..max_bins)
                    .map(|k| vals[k * (vals.len() - 1) / max_bins])
                    .collect();
                c.dedup();
                c
            };
            thresholds.push(cuts);
        }
        let mut bins = vec![0u8; n * f];
        for i in 0..n {
            for j in 0..f {
                let v = data.x[i * f + j];
                bins[i * f + j] = thresholds[j].partition_point(|t| *t < v) as u8;
            }
        }
        Binned {
            bins,
            thresholds,
            features: f,
        }
    }

    fn row(&self, i: usize) -> &[u8] {
        &self.bins[i * self.features..(i + 1) * self.features]
    }
}

fn weighted_quantile(pairs: &mut [(f64, f64)], q: f64) -> f64 {
    if pairs.is_empty() {
        return 0.0;
    }
    pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let total: f64 = pairs.iter().map(|p| p.1).sum();
    let target = q * total;
    let mut acc = 0.0;
    for (v, w) in pairs.iter() {
        acc += w;
        if acc >= target {
            return *v;
        }
    }
    pairs.last().map(|p| p.0).unwrap_or(0.0)
}

fn fit_quantile(
    data: &Dataset,
    binned: &Binned,
    params: &Params,
    q: f64,
    seed: u64,
) -> QuantileModel {
    let n = data.rows();
    let mut pairs: Vec<(f64, f64)> = data.y.iter().copied().zip(data.w.iter().copied()).collect();
    let base = weighted_quantile(&mut pairs, q);
    let mut pred = vec![base; n];
    let mut trees = Vec::with_capacity(params.rounds);
    let mut rng = Rng::new(seed ^ (q * 1000.0) as u64);
    let mut rows: Vec<usize> = Vec::with_capacity(n);
    for _ in 0..params.rounds {
        rows.clear();
        rows.extend((0..n).filter(|_| rng.uniform() < params.subsample));
        if rows.len() < 10 {
            rows.extend(0..n);
        }
        let grad: Vec<f64> = (0..n)
            .map(|i| if data.y[i] > pred[i] { q } else { q - 1.0 })
            .collect();
        let mut nodes = Vec::new();
        grow(
            data, binned, params, &grad, &pred, q, &mut rows, 0, &mut nodes,
        );
        let tree = Tree(nodes);
        for (i, p) in pred.iter_mut().enumerate() {
            *p += params.learning_rate * tree.predict_binned(binned.row(i));
        }
        trees.push(tree);
    }
    QuantileModel {
        quantile: q,
        base,
        learning_rate: params.learning_rate,
        trees,
    }
}

#[allow(clippy::too_many_arguments)]
fn grow(
    data: &Dataset,
    binned: &Binned,
    params: &Params,
    grad: &[f64],
    pred: &[f64],
    q: f64,
    rows: &mut [usize],
    depth: usize,
    nodes: &mut Vec<Node>,
) -> u32 {
    let id = nodes.len() as u32;
    nodes.push(Node::Leaf(0.0));
    let total_w: f64 = rows.iter().map(|&i| data.w[i]).sum();
    let split = if depth < params.depth && total_w >= 2.0 * params.min_leaf_weight {
        best_split(data, binned, params, grad, rows, total_w)
    } else {
        None
    };
    match split {
        Some((feature, bin)) => {
            let mid = partition(rows, |&i| binned.row(i)[feature] <= bin);
            let (l, r) = rows.split_at_mut(mid);
            let left = grow(data, binned, params, grad, pred, q, l, depth + 1, nodes);
            let right = grow(data, binned, params, grad, pred, q, r, depth + 1, nodes);
            nodes[id as usize] = Node::Split {
                feature: feature as u16,
                threshold: binned.thresholds[feature][bin as usize],
                bin,
                left,
                right,
            };
        }
        None => {
            let mut residuals: Vec<(f64, f64)> = rows
                .iter()
                .map(|&i| (data.y[i] - pred[i], data.w[i]))
                .collect();
            nodes[id as usize] = Node::Leaf(weighted_quantile(&mut residuals, q));
        }
    }
    id
}

fn best_split(
    data: &Dataset,
    binned: &Binned,
    params: &Params,
    grad: &[f64],
    rows: &[usize],
    total_w: f64,
) -> Option<(usize, u8)> {
    let total_g: f64 = rows.iter().map(|&i| data.w[i] * grad[i]).sum();
    let parent = total_g * total_g / total_w;
    let mut best: Option<(f64, usize, u8)> = None;
    let mut hist_g = [0f64; 256];
    let mut hist_w = [0f64; 256];
    for j in 0..data.features {
        let cuts = binned.thresholds[j].len();
        if cuts == 0 {
            continue;
        }
        hist_g[..=cuts].fill(0.0);
        hist_w[..=cuts].fill(0.0);
        for &i in rows {
            let b = binned.row(i)[j] as usize;
            hist_g[b] += data.w[i] * grad[i];
            hist_w[b] += data.w[i];
        }
        let (mut gl, mut wl) = (0.0, 0.0);
        for b in 0..cuts {
            gl += hist_g[b];
            wl += hist_w[b];
            let wr = total_w - wl;
            if wl < params.min_leaf_weight || wr < params.min_leaf_weight {
                continue;
            }
            let gr = total_g - gl;
            let gain = gl * gl / wl + gr * gr / wr - parent;
            if gain > 1e-9 && best.is_none_or(|(g, _, _)| gain > g) {
                best = Some((gain, j, b as u8));
            }
        }
    }
    best.map(|(_, j, b)| (j, b))
}

fn partition<T, F: Fn(&T) -> bool>(v: &mut [T], pred: F) -> usize {
    let mut i = 0;
    for j in 0..v.len() {
        if pred(&v[j]) {
            v.swap(i, j);
            i += 1;
        }
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world(n: usize, seed: u64) -> Dataset {
        let mut rng = Rng::new(seed);
        let mut d = Dataset::new(3);
        for _ in 0..n {
            let a = (rng.uniform() * 4.0) as f32;
            let b = if rng.uniform() < 0.3 {
                -1.0
            } else {
                rng.uniform() as f32
            };
            let noise = rng.uniform() as f32;
            let y = 2.0 + a as f64 * 0.8 + if b < 0.0 { 1.0 } else { 0.0 } + 0.3 * rng.normal();
            d.push(&[a, b, noise], y, 1.0);
        }
        d
    }

    #[test]
    fn learns_structure_and_calibrated_quantiles() {
        let train = world(3000, 1);
        let test = world(2000, 2);
        let m = Gbm::fit(&train, &Params::default(), 7).unwrap();
        let (mut below10, mut below90, mut abs) = (0, 0, 0.0);
        for i in 0..test.rows() {
            let p = m.predict(test.row(i));
            assert!(p[0] <= p[1] && p[1] <= p[2]);
            below10 += usize::from(test.y[i] < p[0]);
            below90 += usize::from(test.y[i] < p[2]);
            abs += (test.y[i] - p[1]).abs();
        }
        let n = test.rows() as f64;
        let mae = abs / n;
        assert!(mae < 0.32, "median abs error {mae}");
        let lo = below10 as f64 / n;
        let hi = below90 as f64 / n;
        assert!((0.05..0.16).contains(&lo), "q10 coverage {lo}");
        assert!((0.84..0.95).contains(&hi), "q90 coverage {hi}");
    }

    #[test]
    fn too_little_data_is_refused() {
        assert!(Gbm::fit(&world(5, 3), &Params::default(), 1).is_none());
    }
}
