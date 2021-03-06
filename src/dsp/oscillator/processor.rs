use crate::{
    commands::id::Id,
    graph::dsp::{DspParameterMap, DspProcessor},
    AudioBuffer, SampleLocation, Timestamp,
};

pub struct OscillatorDspProcess {
    phase: f64,
    frequency_id: Id,
    gain_id: Id,
}

lazy_static! {
    static ref SINE_WAVE_TABLE: Vec<f64> = {
        let length = 8192;
        let mut values = Vec::with_capacity(length);

        for frame in 0..length {
            let time = frame as f64 / length as f64;
            let value = (std::f64::consts::TAU * time).sin();
            values.push(value);
        }

        values
    };
}

impl OscillatorDspProcess {
    pub fn new(frequency_id: Id, gain_id: Id) -> Self {
        // ensure table is initialised off the realtime thread
        let _ = SINE_WAVE_TABLE[0];

        Self {
            phase: 0.0,
            frequency_id,
            gain_id,
        }
    }

    fn increment_phase(&mut self, frequency: f64, sample_rate: usize) {
        self.phase += frequency / (sample_rate as f64);
        while self.phase > 1.0 {
            self.phase -= 1.0;
        }
    }

    fn get_value(&self) -> f64 {
        let offset = self.phase * SINE_WAVE_TABLE.len() as f64;

        let offset_before = offset.floor() as usize;
        let offset_after = offset.ceil() as usize;

        let value_before = SINE_WAVE_TABLE[offset_before];
        let value_after = if offset_after < SINE_WAVE_TABLE.len() {
            SINE_WAVE_TABLE[offset_after]
        } else {
            0.0
        };

        let weighting = offset - offset.floor();
        interpolate(value_before, value_after, weighting)
    }
}

fn interpolate(a: f64, b: f64, amount_of_b: f64) -> f64 {
    (1.0 - amount_of_b) * a + amount_of_b * b
}

impl DspProcessor for OscillatorDspProcess {
    fn process_audio(
        &mut self,
        _input_buffer: &dyn AudioBuffer,
        output_buffer: &mut dyn AudioBuffer,
        start_time: &Timestamp,
        parameters: &DspParameterMap,
    ) {
        let sample_rate = output_buffer.sample_rate();

        let frequency = match parameters.get(&self.frequency_id) {
            Some(param) => param,
            None => return,
        };

        let gain = match parameters.get(&self.gain_id) {
            Some(param) => param,
            None => return,
        };

        let num_frames = output_buffer.num_frames();
        let num_channels = output_buffer.num_channels();

        for frame in 0..num_frames {
            let frame_time = start_time.incremented_by_samples(frame, sample_rate);
            let frequency = frequency.get_value_at_time(&frame_time);
            let gain = gain.get_value_at_time(&frame_time);

            self.increment_phase(frequency, sample_rate);
            let value = gain * self.get_value();

            for channel in 0..num_channels {
                output_buffer.set_sample(SampleLocation::new(channel, frame), value as f32);
            }
        }
    }
}
