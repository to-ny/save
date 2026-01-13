//! Traffic pattern generators.

use rand::Rng;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub enum TrafficPattern {
    Baseline {
        requests_per_second: f64,
    },
    BusinessHours {
        peak_rps: f64,
        duration_secs: u64,
        start_time: Instant,
    },
    Bursty {
        base_rps: f64,
        spike_rps: f64,
        spike_probability: f64,
    },
    Mixed {
        read_ratio: f64,
        write_ratio: f64,
        rps: f64,
    },
}

impl TrafficPattern {
    pub fn current_rps(&self) -> f64 {
        match self {
            TrafficPattern::Baseline {
                requests_per_second,
            } => *requests_per_second,
            TrafficPattern::BusinessHours {
                peak_rps,
                duration_secs,
                start_time,
            } => {
                let elapsed = start_time.elapsed().as_secs_f64();
                let total_duration = *duration_secs as f64;

                // Divide into 3 phases: ramp up (20%), steady (60%), ramp down (20%)
                let ramp_up_end = total_duration * 0.2;
                let steady_end = total_duration * 0.8;

                if elapsed < ramp_up_end {
                    // Ramp up phase
                    let progress = elapsed / ramp_up_end;
                    peak_rps * progress
                } else if elapsed < steady_end {
                    // Steady phase
                    *peak_rps
                } else if elapsed < total_duration {
                    // Ramp down phase
                    let progress = (elapsed - steady_end) / (total_duration - steady_end);
                    peak_rps * (1.0 - progress)
                } else {
                    // Pattern complete, restart
                    0.1 // Minimal baseline
                }
            }
            TrafficPattern::Bursty {
                base_rps,
                spike_rps,
                spike_probability,
            } => {
                let mut rng = rand::rng();
                if rng.random::<f64>() < *spike_probability {
                    *spike_rps
                } else {
                    *base_rps
                }
            }
            TrafficPattern::Mixed { rps, .. } => *rps,
        }
    }

    pub fn request_delay(&self) -> Duration {
        let rps = self.current_rps();
        if rps <= 0.0 {
            Duration::from_secs(1)
        } else {
            Duration::from_secs_f64(1.0 / rps)
        }
    }

    pub fn should_read(&self) -> bool {
        match self {
            TrafficPattern::Mixed {
                read_ratio,
                write_ratio,
                ..
            } => {
                let mut rng = rand::rng();
                let total = read_ratio + write_ratio;
                let normalized_read = read_ratio / total;
                rng.random::<f64>() < normalized_read
            }
            _ => {
                // For non-mixed patterns, 50/50 split
                let mut rng = rand::rng();
                rng.random::<f64>() < 0.5
            }
        }
    }

    pub fn reset(&mut self) {
        if let TrafficPattern::BusinessHours { start_time, .. } = self {
            *start_time = Instant::now();
        }
    }
}

impl From<&crate::config::PatternConfig> for TrafficPattern {
    fn from(config: &crate::config::PatternConfig) -> Self {
        match config {
            crate::config::PatternConfig::Baseline {
                requests_per_second,
            } => TrafficPattern::Baseline {
                requests_per_second: *requests_per_second,
            },
            crate::config::PatternConfig::BusinessHours {
                peak_rps,
                duration_secs,
            } => TrafficPattern::BusinessHours {
                peak_rps: *peak_rps,
                duration_secs: *duration_secs,
                start_time: Instant::now(),
            },
            crate::config::PatternConfig::Bursty {
                base_rps,
                spike_rps,
                spike_probability,
            } => TrafficPattern::Bursty {
                base_rps: *base_rps,
                spike_rps: *spike_rps,
                spike_probability: *spike_probability,
            },
            crate::config::PatternConfig::Mixed => TrafficPattern::Mixed {
                read_ratio: 0.7,
                write_ratio: 0.3,
                rps: 5.0,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_baseline_pattern() {
        let pattern = TrafficPattern::Baseline {
            requests_per_second: 10.0,
        };
        assert!((pattern.current_rps() - 10.0).abs() < 0.001);
    }

    #[test]
    fn test_request_delay() {
        let pattern = TrafficPattern::Baseline {
            requests_per_second: 10.0,
        };
        let delay = pattern.request_delay();
        assert!((delay.as_secs_f64() - 0.1).abs() < 0.001);
    }

    #[test]
    fn test_mixed_pattern_ratios() {
        let pattern = TrafficPattern::Mixed {
            read_ratio: 1.0,
            write_ratio: 0.0,
            rps: 5.0,
        };
        // With 100% read ratio, should_read should always return true
        for _ in 0..100 {
            assert!(pattern.should_read());
        }

        let pattern = TrafficPattern::Mixed {
            read_ratio: 0.0,
            write_ratio: 1.0,
            rps: 5.0,
        };
        // With 0% read ratio, should_read should always return false
        for _ in 0..100 {
            assert!(!pattern.should_read());
        }
    }

    #[test]
    fn test_bursty_pattern() {
        let pattern = TrafficPattern::Bursty {
            base_rps: 5.0,
            spike_rps: 50.0,
            spike_probability: 0.0,
        };
        // With 0% spike probability, should always return base_rps
        for _ in 0..100 {
            assert!((pattern.current_rps() - 5.0).abs() < 0.001);
        }
    }
}
