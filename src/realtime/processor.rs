use crate::{
    audio_process::AudioProcess,
    buffer::{audio_buffer::AudioBuffer, audio_buffer_slice::AudioBufferSlice},
    commands::{command::Command, notification::Notification},
    timestamp::Timestamp,
};
use lockfree::channel::{mpsc::Receiver, spsc::Sender};

use super::{dsp_graph::DspGraph, periodic_notification::PeriodicNotification};

const MAXIMUM_NUMBER_OF_FRAMES: usize = 512;
const MAXIMUM_NUMBER_OF_CHANNELS: usize = 2;
const POSITION_INTERVAL_HZ: f64 = 30.0;

pub struct Processor {
    started: bool,
    sample_rate: usize,
    command_rx: Receiver<Command>,
    notification_tx: Sender<Notification>,

    sample_position: usize,
    graph: DspGraph,

    position_notification: PeriodicNotification,
}

impl Processor {
    pub fn new(
        sample_rate: usize,
        command_rx: Receiver<Command>,
        notification_tx: Sender<Notification>,
    ) -> Self {
        Self {
            started: false,
            sample_rate,
            command_rx,
            notification_tx,
            sample_position: 0,
            graph: DspGraph::new(
                MAXIMUM_NUMBER_OF_FRAMES,
                MAXIMUM_NUMBER_OF_CHANNELS,
                sample_rate,
            ),
            position_notification: PeriodicNotification::new(sample_rate, POSITION_INTERVAL_HZ),
        }
    }

    fn process_graph(&mut self, output_buffer: &mut dyn AudioBuffer) {
        let current_time = self.current_time();

        let mut offset = 0;

        while offset < output_buffer.num_frames() {
            let num_frames = std::cmp::min(
                output_buffer.num_frames() - offset,
                self.get_maximum_number_of_frames(),
            );

            let mut audio_buffer = AudioBufferSlice::new(output_buffer, offset, num_frames);

            self.graph.process(&mut audio_buffer, &current_time);

            offset += num_frames;
        }
    }
}

impl AudioProcess for Processor {
    fn process(&mut self, output_buffer: &mut dyn AudioBuffer) {
        output_buffer.clear();

        self.process_commands();

        if !self.started {
            return;
        }

        let num_frames = output_buffer.num_frames();
        self.process_graph(output_buffer);
        self.update_position(num_frames);
        self.notify_position(num_frames);
    }
}

impl Processor {
    fn process_commands(&mut self) {
        while let Ok(command) = self.command_rx.recv() {
            match command {
                Command::Start => self.started = true,
                Command::Stop => self.started = false,

                Command::AddDsp(dsp) => self.graph.add_dsp(dsp),
                Command::RemoveDsp(id) => self.graph.remove_dsp(id),

                Command::ParameterValueChange(change_request) => {
                    self.graph.request_parameter_change(change_request)
                }

                Command::AddConnection(connection) => self.graph.add_connection(connection),
                Command::RemoveConnection(connection) => self.graph.remove_connection(connection),
                Command::ConnectToOutput(output_connection) => {
                    self.graph.connect_to_output(output_connection)
                }
            }
        }
    }

    fn get_maximum_number_of_frames(&self) -> usize {
        MAXIMUM_NUMBER_OF_FRAMES
    }

    fn send_notficiation(&mut self, notification: Notification) {
        let _ = self.notification_tx.send(notification);
    }

    fn update_position(&mut self, num_samples: usize) {
        self.sample_position += num_samples;
    }

    fn current_time(&self) -> Timestamp {
        Timestamp::from_seconds(self.sample_position as f64 / self.sample_rate as f64)
    }

    fn notify_position(&mut self, num_samples: usize) {
        if self.position_notification.increment(num_samples) {
            self.send_notficiation(Notification::Position(self.current_time()));
        }
    }
}
