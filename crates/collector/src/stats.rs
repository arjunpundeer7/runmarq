//! Fixed-size latency sample buffers for diagnostics.

use runmarq_core::api::LatencyStats;
use std::collections::VecDeque;

const CAPACITY: usize = 512;

#[derive(Debug, Default, Clone)]
pub struct Samples {
    buf: VecDeque<f64>,
}

impl Samples {
    pub fn push(&mut self, ms: f64) {
        if !ms.is_finite() {
            return;
        }
        if self.buf.len() == CAPACITY {
            self.buf.pop_front();
        }
        self.buf.push_back(ms);
    }

    pub fn stats(&self) -> LatencyStats {
        let mut v: Vec<f64> = self.buf.iter().copied().collect();
        let n = v.len() as u32;
        LatencyStats {
            samples: n,
            p50_ms: runmarq_estimator::stats::quantile(&mut v, 0.5),
            p95_ms: runmarq_estimator::stats::quantile(&mut v, 0.95),
            max_ms: v.last().copied(),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn bounded_and_ordered() {
        let mut s = super::Samples::default();
        for i in 0..1000 {
            s.push(i as f64);
        }
        let st = s.stats();
        assert_eq!(st.samples, 512);
        assert_eq!(st.max_ms, Some(999.0));
        assert!(st.p50_ms.unwrap() < st.p95_ms.unwrap());
    }
}
