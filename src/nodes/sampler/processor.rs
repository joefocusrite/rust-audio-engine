use crate::{
    graph::dsp::{DspParameterMap, DspProcessor},
    AudioBuffer, OwnedAudioBuffer, Timestamp,
};

use super::{fade::Fade, voice::Voice};

pub struct SamplerDspProcess {
    fade: Fade,
    voices: Vec<Voice>,
    active_voice: Option<usize>,
    buffer: OwnedAudioBuffer,
}

const NUM_VOICES: usize = 4;
const FADE_LENGTH_MS: f64 = 50.0;

impl DspProcessor for SamplerDspProcess {
    fn process_audio(
        &mut self,
        _input_buffer: &dyn AudioBuffer,
        output_buffer: &mut dyn AudioBuffer,
        _start_time: &Timestamp,
        _parameters: &DspParameterMap,
    ) {
        let fade = &self.fade;
        let sample = &self.buffer;
        self.voices
            .iter_mut()
            .for_each(|voice| voice.render(output_buffer, sample, fade));
    }
}

impl SamplerDspProcess {
    pub fn new(sample_rate: usize, buffer: OwnedAudioBuffer) -> Self {
        Self {
            fade: Fade::new(FADE_LENGTH_MS, sample_rate),
            voices: (0..NUM_VOICES).map(|_| Voice::default()).collect(),
            active_voice: None,
            buffer,
        }
    }

    fn assign_voice(&mut self, position: usize) {
        self.stop();

        if let Some((index, free_voice)) = self
            .voices
            .iter_mut()
            .enumerate()
            .find(|(_, voice)| voice.is_stopped())
        {
            free_voice.start_from_position(position);
            self.active_voice = Some(index);
        }
    }

    fn get_active_voice(&self) -> Option<&Voice> {
        if let Some(active_voice_index) = self.active_voice {
            return self.voices.get(active_voice_index);
        }

        None
    }

    fn get_active_voice_position(&self) -> Option<usize> {
        self.get_active_voice().map(|voice| voice.get_position())
    }

    pub fn start(&mut self, from_position: usize) {
        if let Some(current_position) = self.get_active_voice_position() {
            if current_position == from_position {
                return;
            }
        }

        self.assign_voice(from_position);
    }

    pub fn stop(&mut self) {
        self.voices.iter_mut().for_each(|voice| voice.stop());
        self.active_voice = None
    }
}

#[cfg(test)]
mod tests {
    use crate::{OwnedAudioBuffer, SampleLocation};

    use super::*;

    fn create_sample_with_value(
        num_frames: usize,
        num_channels: usize,
        sample_rate: usize,
        value: f32,
    ) -> OwnedAudioBuffer {
        let mut sample = OwnedAudioBuffer::new(num_frames, num_channels, sample_rate);
        sample.fill_with_value(value);
        sample
    }

    fn process_sampler(
        sampler: &mut SamplerDspProcess,
        num_frames: usize,
        num_channels: usize,
        sample_rate: usize,
    ) -> OwnedAudioBuffer {
        let mut output_buffer = OwnedAudioBuffer::new(num_frames, num_channels, sample_rate);
        let input_buffer = OwnedAudioBuffer::new(num_frames, num_channels, sample_rate);
        let start_time = Timestamp::from_seconds(0.0);

        sampler.process_audio(
            &input_buffer,
            &mut output_buffer,
            &start_time,
            &DspParameterMap::new(),
        );

        output_buffer
    }

    #[test]
    fn fades_in() {
        let num_frames = 10_000;
        let sample_rate = 44_100;
        let num_channels = 1;

        let sample = create_sample_with_value(num_frames, num_channels, sample_rate, 1.0);
        let mut sampler = SamplerDspProcess::new(sample_rate, sample);

        sampler.start(0);

        let output_buffer = process_sampler(&mut sampler, num_frames, num_channels, sample_rate);

        approx::assert_relative_eq!(
            0.0,
            output_buffer.get_sample(&SampleLocation {
                frame: 0,
                channel: 0
            })
        );
        approx::assert_relative_eq!(
            0.5,
            output_buffer.get_sample(&SampleLocation {
                frame: sampler.fade.len() / 2,
                channel: 0
            }),
            epsilon = 0.01
        );
        approx::assert_relative_eq!(
            1.0,
            output_buffer.get_sample(&SampleLocation {
                frame: sampler.fade.len(),
                channel: 0
            }),
            epsilon = 0.01
        );
    }

    #[test]
    fn fades_out() {
        let num_frames = 10_000;
        let sample_rate = 44_100;
        let num_channels = 1;

        let sample = create_sample_with_value(num_frames, num_channels, sample_rate, 1.0);
        let mut sampler = SamplerDspProcess::new(sample_rate, sample);

        sampler.start(0);

        let fade_length = sampler.fade.len();

        let _ = process_sampler(&mut sampler, 2 * fade_length, num_channels, sample_rate);
        sampler.stop();
        let output = process_sampler(&mut sampler, 2 * fade_length, num_channels, sample_rate);

        approx::assert_relative_eq!(
            1.0,
            output.get_sample(&SampleLocation {
                frame: 0,
                channel: 0
            })
        );

        approx::assert_relative_eq!(
            0.5,
            output.get_sample(&SampleLocation {
                frame: sampler.fade.len() / 2,
                channel: 0
            }),
            epsilon = 0.01
        );

        approx::assert_relative_eq!(
            0.0,
            output.get_sample(&SampleLocation {
                frame: sampler.fade.len(),
                channel: 0
            }),
            epsilon = 0.01
        );
    }

    #[test]
    fn fade_out_beyond_sample() {
        let num_frames = 10_000;
        let sample_rate = 48_000;
        let num_channels = 2;

        let sample = create_sample_with_value(num_frames, num_channels, sample_rate, 1.0);
        let mut sampler = SamplerDspProcess::new(sample_rate, sample);
        sampler.start(0);

        let fade_length = sampler.fade.len();

        let _ = process_sampler(
            &mut sampler,
            num_frames - fade_length / 2,
            num_channels,
            sample_rate,
        );

        sampler.stop();

        let output = process_sampler(&mut sampler, 2 * fade_length, num_channels, sample_rate);

        approx::assert_relative_eq!(
            1.0,
            output.get_sample(&SampleLocation {
                frame: 0,
                channel: 0
            })
        );

        approx::assert_relative_eq!(
            0.0,
            output.get_sample(&SampleLocation {
                frame: sampler.fade.len(),
                channel: 0
            }),
            epsilon = 0.01
        );
    }
}
