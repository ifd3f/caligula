use std::{fmt::Display, time::Instant};

use bytesize::ByteSize;

use crate::ui::utils::ByteSpeed;

pub enum EstimatedTime {
    Known(f64),
    Unknown,
}

#[derive(Debug)]
pub struct ByteSeries {
    max_bytes: ByteSize,
    raw: Vec<(f64, u64)>,
    start: Instant,
}

impl From<f64> for EstimatedTime {
    fn from(value: f64) -> Self {
        if value.is_finite() {
            Self::Known(value)
        } else {
            Self::Unknown
        }
    }
}

impl Display for EstimatedTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EstimatedTime::Known(x) => write!(f, "{x:.1}s"),
            EstimatedTime::Unknown => write!(f, "[unknown]"),
        }
    }
}

impl ByteSeries {
    pub fn new(start: Instant, max_bytes: ByteSize) -> Self {
        Self {
            start,
            max_bytes,
            raw: vec![(0.0, 0)],
        }
    }

    pub fn push(&mut self, time: Instant, bytes: u64) {
        let secs = time.duration_since(self.start).as_secs_f64();
        self.raw.push((secs, bytes));
    }

    pub fn finished_verifying_at(&mut self, time: Instant) {
        self.push(time, self.max_bytes.0);
    }

    pub fn last_datapoint(&self) -> (f64, ByteSize) {
        self.raw
            .last()
            .map(|(x, y)| (*x, ByteSize::b(*y)))
            .unwrap_or((0.0, ByteSize::b(0)))
    }

    pub fn bytes_written(&self) -> ByteSize {
        self.last_datapoint().1
    }

    pub fn total_avg_speed(&self) -> ByteSpeed {
        let s = self.bytes_written();
        let speed = s.0 as f64 / self.last_datapoint().0;
        ByteSpeed(if speed.is_nan() { 0.0 } else { speed })
    }

    pub fn estimated_time_left(&self) -> EstimatedTime {
        let speed = self.total_avg_speed().0;
        let bytes_left = self.max_bytes().0 - self.bytes_written().0;
        let secs_left = bytes_left as f64 / speed;
        EstimatedTime::from(secs_left)
    }

    pub fn max_bytes(&self) -> ByteSize {
        self.max_bytes
    }

    pub fn start(&self) -> Instant {
        self.start
    }

    pub fn speed(&self, t: f64, window: f64) -> f64 {
        let b0 = self.interp_bytes(t - window);
        let b1 = self.interp_bytes(t);

        (b1 - b0) / window
    }

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

    /// Returns the interpolated number of bytes written at the given time.
    pub fn interp_bytes(&self, t: f64) -> f64 {
        if t < 0.0 {
            return self.raw[0].1 as f64;
        }
        let (last, last_val) = self.last_datapoint();
        if t >= last {
            return last_val.as_u64() as f64;
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

    use approx::assert_relative_eq;
    use bytesize::ByteSize;

    use super::ByteSeries;
    use test_case::test_case;

    fn example_2s() -> ByteSeries {
        let now = Instant::now();
        let mut s = ByteSeries::new(now, ByteSize::b(100));
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
        let actual = example_2s().interp_bytes(t);
        assert_relative_eq!(actual, expected);
    }
}
