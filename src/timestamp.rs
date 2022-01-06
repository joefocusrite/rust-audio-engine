#[derive(Clone, Copy)]
pub struct Timestamp {
    seconds: f64,
}

impl Default for Timestamp {
    fn default() -> Self {
        Self { seconds: 0.0 }
    }
}

impl Timestamp {
    pub fn with_seconds(seconds: f64) -> Self {
        Self { seconds }
    }

    pub fn get_seconds(&self) -> f64 {
        self.seconds
    }

    pub fn incremented(&self, num_samples: usize, sample_rate: usize) -> Self {
        Self {
            seconds: self.seconds + num_samples as f64 / sample_rate as f64,
        }
    }
}

#[cfg(test)]
mod tests {
    use approx::assert_relative_eq;

    use super::*;
    #[test]
    fn it_increments() {
        let sample_rate = 44100;
        let before = Timestamp::default();
        let after = before.incremented(sample_rate, sample_rate);
        assert_relative_eq!(after.get_seconds() - before.get_seconds(), 1.0);
    }
}