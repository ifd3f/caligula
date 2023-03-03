use std::{fmt::Display, time::Instant};

use bytesize::ByteSize;

use crate::ui::utils::ByteSpeed;

pub enum EstimatedTime {
    Known(f64),
    Unknown,
}

pub struct ByteSeries {
    max_bytes: ByteSize,
    raw: Vec<(f64, ByteSize)>,
    speed_data: Vec<(f64, f64)>,
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
            raw: vec![],
            speed_data: vec![],
        }
    }

    pub fn push(&mut self, time: Instant, bytes: u64) {
        let secs = time.duration_since(self.start).as_secs_f64();
        let (last_time, last_bw) = self.last_datapoint();
        let dt = secs - last_time;
        let diff = bytes - last_bw.0;
        let speed = diff as f64 / dt;

        self.raw.push((secs, ByteSize::b(bytes)));
        self.speed_data.push((secs, speed));
    }

    pub fn finished_verifying_at(&mut self, time: Instant) {
        self.push(time, self.max_bytes.0);
    }

    pub fn last_datapoint(&self) -> (f64, ByteSize) {
        self.raw
            .last()
            .map(|x| x.clone())
            .unwrap_or((0.0, ByteSize::b(0)))
    }

    pub fn bytes_written(&self) -> ByteSize {
        self.last_datapoint().1
    }

    pub fn total_avg_speed(&self, final_time: Instant) -> ByteSpeed {
        let s = self.bytes_written();
        let dt = final_time.duration_since(self.start).as_secs_f64();
        let speed = s.0 as f64 / dt;
        ByteSpeed(if speed.is_nan() { 0.0 } else { speed })
    }

    pub fn estimated_time_left(&self, final_time: Instant) -> EstimatedTime {
        let speed = self.total_avg_speed(final_time).0;
        let bytes_left = self.max_bytes().0 - self.bytes_written().0;
        let secs_left = bytes_left as f64 / speed;
        EstimatedTime::from(secs_left)
    }

    pub fn last_speed_data(&self) -> (f64, f64) {
        self.speed_data
            .last()
            .map(|x| x.clone())
            .unwrap_or((0.0, 0.0))
    }

    pub fn last_speed(&self) -> ByteSpeed {
        let (_, s) = self.last_speed_data();
        ByteSpeed(s)
    }

    pub fn max_speed(&self) -> ByteSpeed {
        ByteSpeed(self.speed_data.iter().map(|x| x.1).fold(0.0, f64::max))
    }

    pub fn max_bytes(&self) -> ByteSize {
        self.max_bytes
    }

    pub fn start(&self) -> Instant {
        self.start
    }

    pub fn speed_data(&self) -> &[(f64, f64)] {
        self.speed_data.as_ref()
    }
}
