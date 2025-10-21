use crate::ui::utils::ByteSpeed;
use chrono::Datelike;
use std::ops::Add;
use std::{fmt::Display, time::Instant};

#[derive(Debug, Clone, PartialEq)]
pub struct EstimatedTimeInfo {
    secs_left: f64,
    now: chrono::DateTime<chrono::Local>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EstimatedTime {
    Known(EstimatedTimeInfo),
    Unknown,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ByteSeries {
    raw: Vec<(f64, u64)>,
    start: Instant,
}

impl From<EstimatedTimeInfo> for EstimatedTime {
    fn from(value: EstimatedTimeInfo) -> Self {
        if value.secs_left.is_finite() {
            Self::Known(value)
        } else {
            Self::Unknown
        }
    }
}

impl Display for EstimatedTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EstimatedTime::Known(x) => {
                let rounded = x.secs_left.round();
                let secs = rounded % 60.0;
                let mins = (rounded / 60.0) % 60.0;
                let hours = rounded / 3600.0;

                let completion_time = x.now.add(chrono::TimeDelta::seconds(rounded as i64));

                // Show the whole date only if completion time is at least a day ahead of now
                let completion_time_format = if hours >= 24.0 {
                    "%Y-%m-%d %H:%M:%S"
                } else {
                    "%H:%M:%S"
                };

                write!(
                    f,
                    "{:0>2}:{:0>2}:{:0>2} (complete at {})",
                    hours as u64,
                    mins as u8,
                    secs as u8,
                    completion_time.format(completion_time_format)
                )
            }
            EstimatedTime::Unknown => write!(f, "[unknown]"),
        }
    }
}

impl ByteSeries {
    pub fn new(start: Instant) -> Self {
        Self {
            start,
            raw: vec![(0.0, 0)],
        }
    }

    pub fn push(&mut self, time: Instant, bytes: u64) {
        let secs = time.duration_since(self.start).as_secs_f64();
        self.raw.push((secs, bytes));
    }

    pub fn last_datapoint(&self) -> (f64, u64) {
        self.raw.last().copied().unwrap_or((0.0, 0))
    }

    pub fn bytes_encountered(&self) -> u64 {
        self.last_datapoint().1
    }

    pub fn total_avg_speed(&self) -> ByteSpeed {
        let s = self.bytes_encountered();
        let speed = s as f64 / self.last_datapoint().0;
        ByteSpeed(if speed.is_nan() { 0.0 } else { speed })
    }

    pub fn estimated_time_left(&self, total_bytes: u64) -> EstimatedTime {
        let speed = self.total_avg_speed().0;
        // Saturating subtract is necessary because bytes encountered may be greater
        // than total bytes, due to the nature of block writing.
        let bytes_left = total_bytes.saturating_sub(self.bytes_encountered());
        let secs_left = bytes_left as f64 / speed;
        EstimatedTime::from(EstimatedTimeInfo {
            secs_left,
            now: chrono::Local::now(),
        })
    }

    pub fn start(&self) -> Instant {
        self.start
    }

    pub fn speed(&self, t: f64, window: f64) -> f64 {
        let b0 = self.interp(t - window);
        let b1 = self.interp(t);

        (b1 - b0) / window
    }

    /// Returns a series of points representing a timeseries, aggregated by the given window size.
    pub fn speeds(&self, window: f64) -> impl Iterator<Item = (f64, f64)> + '_ {
        let bins = (self.last_datapoint().0 / window).ceil() as usize;
        (0..bins).map(move |i| {
            let t = i as f64 * window;
            (t, self.speed(t, window))
        })
    }

    /// Returns the index of the sample right before the requested time.
    fn find_idx_below(&self, t: f64) -> usize {
        let mut min = 0;
        if t <= self.raw[min].0 {
            return min;
        }

        let mut max = self.raw.len() - 1;
        if self.raw[max].0 <= t {
            return max;
        }

        loop {
            if min >= max - 1 {
                return min;
            }
            let mid = (min + max) / 2;
            let mid_val = self.raw[mid].0;
            if t < mid_val {
                max = mid;
            } else {
                min = mid;
            }
        }
    }

    /// Returns the interpolated number of bytes encountered at the given time.
    pub fn interp(&self, t: f64) -> f64 {
        if t < 0.0 {
            return self.raw[0].1 as f64;
        }
        let (last, last_val) = self.last_datapoint();
        if t >= last {
            return last_val as f64;
        }

        let i0 = self.find_idx_below(t);
        let i1 = i0 + 1;
        let (x0, y0) = self.raw[i0];
        let (x1, y1) = self.raw[i1];

        (y1 as f64 - y0 as f64) * (t - x0) / (x1 - x0) + y0 as f64
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::EstimatedTime;
    use super::{ByteSeries, EstimatedTimeInfo};
    use approx::assert_relative_eq;
    use chrono::{Local, TimeZone, Utc};
    use test_case::test_case;

    fn example_2s() -> ByteSeries {
        let now = Instant::now();
        let mut s = ByteSeries::new(now);
        s.push(now.checked_add(Duration::from_secs_f64(0.1)).unwrap(), 10);
        s.push(now.checked_add(Duration::from_secs_f64(0.2)).unwrap(), 20);
        s.push(now.checked_add(Duration::from_secs_f64(0.5)).unwrap(), 30);
        s.push(now.checked_add(Duration::from_secs_f64(1.0)).unwrap(), 40);
        s.push(now.checked_add(Duration::from_secs_f64(1.5)).unwrap(), 80);
        s.push(now.checked_add(Duration::from_secs_f64(2.0)).unwrap(), 100);
        s
    }

    #[test_case(0.0 => is eq 0; "zero")]
    #[test_case(-10.0 => is eq 0; "negative")]
    #[test_case(0.4 => is eq 2; "between")]
    #[test_case(0.5 => is eq 3; "exact")]
    #[test_case(2.0 => is eq 6; "exactly last")]
    #[test_case(3.0 => is eq 6; "over")]
    fn find_idx_below(t: f64) -> usize {
        example_2s().find_idx_below(t)
    }

    #[test_case(0.0, 0.0; "zero")]
    #[test_case(-10.0, 0.0; "negative")]
    #[test_case(0.75, 35.0; "between")]
    #[test_case(0.5, 30.0; "exact")]
    #[test_case(2.0, 100.0; "exactly last")]
    #[test_case(3.0, 100.0; "over")]
    fn interp_bytes(t: f64, expected: f64) {
        let actual = example_2s().interp(t);
        assert_relative_eq!(actual, expected);
    }

    #[test_case(f64::INFINITY, "[unknown]"; "non finite")]
    #[test_case(39_562.0, "10:59:22 (complete at 20:59:27)"; "less than a day")]
    #[test_case(86_400.0, "24:00:00 (complete at 2025-10-22 10:00:05)"; "exactly a day")]
    #[test_case(133_800.0, "37:10:00 (complete at 2025-10-22 23:10:05)"; "more than a day")]
    #[test_case(60.5, "00:01:01 (complete at 10:01:06)"; "round decimals up")]
    #[test_case(59.4, "00:00:59 (complete at 10:01:04)"; "round decimals down")]
    fn estimated_time_display(secs_left: f64, expected: &str) {
        let now = Local.with_ymd_and_hms(2025, 10, 21, 10, 0, 5).unwrap();
        let actual = EstimatedTime::from(EstimatedTimeInfo { secs_left, now }).to_string();
        assert_eq!(expected, actual);
    }
}
