// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Prometheus text of the `sim` processes (M5 spec section 11.3): counters and histogram
//! buckets, summed over every sim. Only plain sample lines are read: `name value` or
//! `name{label="value",...} value`; comments and other lines are skipped.

/// Two bucket bounds closer than this are the same bound.
const BOUND_TOLERANCE: f64 = 1e-9;
/// Suffix of histogram bucket series.
const BUCKET_SUFFIX: &str = "_bucket";
/// The bound label of a bucket.
const BOUND_LABEL: &str = "le";

#[derive(Debug, Clone, PartialEq)]
struct Sample {
    name: String,
    labels: Vec<(String, String)>,
    value: f64,
}

/// The samples of one or more scrapes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scrape {
    samples: Vec<Sample>,
}

impl Scrape {
    /// The samples of a Prometheus text.
    pub fn parse(text: &str) -> Self {
        Self {
            samples: text.lines().filter_map(sample).collect(),
        }
    }

    /// Adds the samples of `other`.
    pub fn add(&mut self, other: &Self) {
        self.samples.extend(other.samples.iter().cloned());
    }

    /// The sum of the series `name`, only those with `label` when given.
    pub fn total(&self, name: &str, label: Option<(&str, &str)>) -> f64 {
        self.samples
            .iter()
            .filter(|sample| sample.name == name)
            .filter(|sample| {
                label.is_none_or(|(key, value)| {
                    sample.labels.iter().any(|(sample_key, sample_value)| {
                        sample_key == key && sample_value == value
                    })
                })
            })
            .map(|sample| sample.value)
            .sum()
    }

    /// The sum of the bucket of `histogram` with the bound `bound`.
    pub fn bucket(&self, histogram: &str, bound: f64) -> f64 {
        let name = format!("{histogram}{BUCKET_SUFFIX}");
        self.samples
            .iter()
            .filter(|sample| sample.name == name)
            .filter(|sample| {
                sample.labels.iter().any(|(key, value)| {
                    key == BOUND_LABEL
                        && value
                            .parse::<f64>()
                            .is_ok_and(|parsed| (parsed - bound).abs() < BOUND_TOLERANCE)
                })
            })
            .map(|sample| sample.value)
            .sum()
    }
}

/// One sample line, or `None` for any other line.
fn sample(line: &str) -> Option<Sample> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let (series, value) = line.rsplit_once(' ')?;
    let value = value.parse().ok()?;
    let (name, labels) = match series.split_once('{') {
        Some((name, rest)) => (name, labels(rest.strip_suffix('}')?)?),
        None => (series, Vec::new()),
    };
    Some(Sample {
        name: name.to_owned(),
        labels,
        value,
    })
}

/// `key="value"` pairs, comma separated.
fn labels(text: &str) -> Option<Vec<(String, String)>> {
    text.split(',')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (key, value) = pair.split_once('=')?;
            let value = value.strip_prefix('"')?.strip_suffix('"')?;
            Some((key.to_owned(), value.to_owned()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "# TYPE sim_handoffs_total counter
sim_handoffs_total{result=\"moved\"} 12
sim_handoffs_total{result=\"timeout\"} 1
# TYPE sim_handoff_seconds histogram
sim_handoff_seconds_bucket{le=\"0.02\"} 10
sim_handoff_seconds_bucket{le=\"0.033\"} 12
sim_handoff_seconds_bucket{le=\"+Inf\"} 13
sim_handoff_seconds_sum 0.2
sim_handoff_seconds_count 13
sim_players 4
";

    #[test]
    fn histogram_buckets_parse() {
        let scrape = Scrape::parse(TEXT);
        assert_eq!(
            scrape.total("sim_handoffs_total", Some(("result", "moved"))),
            12.0
        );
        assert_eq!(scrape.total("sim_handoffs_total", None), 13.0);
        assert_eq!(scrape.total("sim_players", None), 4.0);
        assert_eq!(scrape.total("missing", None), 0.0);
        assert_eq!(scrape.bucket("sim_handoff_seconds", 0.033), 12.0);
        assert_eq!(scrape.total("sim_handoff_seconds_count", None), 13.0);
    }

    #[test]
    fn scrapes_add_up_and_lines_that_are_not_samples_are_skipped() {
        let mut sum = Scrape::parse(TEXT);
        sum.add(&Scrape::parse(
            "sim_players 6\nnot a sample line\nsim_x{a=\"1\" 3\n",
        ));
        assert_eq!(sum.total("sim_players", None), 10.0);
        assert_eq!(sum.total("sim_x", None), 0.0);
    }
}
