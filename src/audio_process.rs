use crate::buffer::audio_buffer::AudioBuffer;

pub trait AudioProcess {
    fn process(&mut self, data: &mut dyn AudioBuffer);

    fn get_maximum_number_of_frames(&self) -> usize;
    fn get_maximum_number_of_channel(&self) -> usize;
}
