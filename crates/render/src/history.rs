//! Valores recientes de cada métrica, para las gráficas.

use std::collections::{HashMap, VecDeque};

use pulse_shared::metrics::{Metric, Readings};
use pulse_shared::theme::MAX_HISTORY;

#[derive(Default)]
pub struct History {
    series: HashMap<Metric, VecDeque<Option<f32>>>,
}

impl History {
    /// Añade la lectura de cada métrica que usa el tema. Un hueco queda como hueco, no como cero.
    pub fn push(&mut self, metrics: &[Metric], readings: &Readings) {
        for metric in metrics {
            let series = self.series.entry(*metric).or_default();
            if series.len() >= MAX_HISTORY as usize {
                series.pop_front();
            }
            series.push_back(readings.get(*metric).number().map(|n| n as f32));
        }
    }

    /// Historia inventada y suave, para las vistas previas.
    pub fn sample(metrics: &[Metric]) -> Self {
        let mut history = Self::default();
        let mut readings = Readings::samples();
        for i in 0..MAX_HISTORY {
            let t = i as f64 / 9.0;
            for metric in metrics {
                let (low, high) = metric.range().unwrap_or((0.0, 100.0));
                let wave = 0.5 + 0.3 * t.sin() + 0.12 * (t * 2.7).cos();
                readings.set_number(*metric, Some(low + (high - low) * wave));
            }
            history.push(metrics, &readings);
        }
        history
    }

    /// Los últimos `count` valores, del más antiguo al más nuevo.
    pub fn last(&self, metric: Metric, count: usize) -> Vec<Option<f32>> {
        let Some(series) = self.series.get(&metric) else { return Vec::new() };
        series.iter().skip(series.len().saturating_sub(count)).copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulse_shared::metrics::Value;

    fn reading(value: Option<f64>) -> Readings {
        let mut r = Readings::default();
        r.set_number(Metric::CpuUsage, value);
        r
    }

    #[test]
    fn a_missing_reading_is_a_gap_not_a_zero() {
        let mut h = History::default();
        h.push(&[Metric::CpuUsage], &reading(Some(10.0)));
        h.push(&[Metric::CpuUsage], &reading(None));
        assert_eq!(h.last(Metric::CpuUsage, 5), [Some(10.0), None]);
    }

    #[test]
    fn it_never_grows_past_the_limit() {
        let mut h = History::default();
        for i in 0..1000 {
            h.push(&[Metric::CpuUsage], &reading(Some(i as f64)));
        }
        let all = h.last(Metric::CpuUsage, 5000);
        assert_eq!(all.len(), MAX_HISTORY as usize);
        assert_eq!(all.last(), Some(&Some(999.0)));
        assert!(matches!(reading(None).get(Metric::CpuUsage), Value::Missing));
    }
}
