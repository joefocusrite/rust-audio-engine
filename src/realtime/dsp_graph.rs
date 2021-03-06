use lockfree::channel::{spsc, spsc::Sender};

use crate::{
    buffer::{
        audio_buffer::AudioBuffer, audio_buffer_slice::AudioBufferSlice,
        sample_location::SampleLocation,
    },
    commands::{command::ParameterChangeRequest, id::Id},
    graph::{
        buffer_pool::BufferPool,
        connection::Connection,
        dsp::Dsp,
        endpoint::{Endpoint, EndpointType},
    },
    timestamp::Timestamp,
};

use super::{
    garbage_collector::{run_garbage_collector, GarbageCollectionCommand},
    graph::{Direction, Graph},
    topological_sort::TopologicalSort,
};

pub struct DspGraph {
    graph: Graph<Box<Dsp>, Connection>,
    topological_sort: TopologicalSort,
    output_endpoint: Option<Endpoint>,
    garbase_collection_tx: Sender<GarbageCollectionCommand>,
    graph_needs_sort: bool,
    buffer_pool: BufferPool,
    maximum_number_of_channels: usize,
    maximum_number_of_frames: usize,
}

impl DspGraph {
    pub fn new(
        maximum_number_of_frames: usize,
        maximum_number_of_channels: usize,
        sample_rate: usize,
    ) -> Self {
        let (garbase_collection_tx, garbage_collection_rx) = spsc::create();
        run_garbage_collector(garbage_collection_rx);

        Self {
            graph: Graph::with_capacity(512, 512),
            topological_sort: TopologicalSort::with_capacity(512),
            graph_needs_sort: false,
            output_endpoint: None,
            garbase_collection_tx,
            buffer_pool: BufferPool::with_capacity(
                128,
                maximum_number_of_frames,
                maximum_number_of_channels,
                sample_rate,
            ),
            maximum_number_of_channels,
            maximum_number_of_frames,
        }
    }

    pub fn process(&mut self, output_buffer: &mut dyn AudioBuffer, start_time: &Timestamp) {
        let num_channels = std::cmp::min(
            output_buffer.num_channels(),
            self.maximum_number_of_channels,
        );
        let num_frames = std::cmp::min(output_buffer.num_frames(), self.maximum_number_of_frames);

        self.sort_graph();
        self.process_dsps(num_frames, num_channels, start_time);
        self.write_to_output(output_buffer, num_channels, num_frames);

        self.buffer_pool.clear_assignments();
        assert!(self.buffer_pool.all_buffers_are_available())
    }

    pub fn add_dsp(&mut self, dsp: Box<Dsp>) {
        let id = dsp.get_id();
        self.graph.add_node_with_id(id, dsp);
        self.mark_graph_needs_sort();
    }

    fn mark_graph_needs_sort(&mut self) {
        self.graph_needs_sort = true;
    }

    fn sort_graph(&mut self) {
        if self.graph_needs_sort {
            self.topological_sort.sort(&self.graph);
            self.graph_needs_sort = false;
        }
    }

    pub fn remove_dsp(&mut self, id: Id) {
        if let Some(dsp) = self.graph.remove_node(id) {
            let _ = self
                .garbase_collection_tx
                .send(GarbageCollectionCommand::DisposeDsp(dsp));
        }

        self.mark_graph_needs_sort();
    }

    pub fn request_parameter_change(&mut self, change_request: ParameterChangeRequest) {
        if let Some(dsp) = self.graph.get_node_mut(change_request.dsp_id) {
            dsp.request_parameter_change(change_request);
        }
    }

    pub fn add_connection(&mut self, connection: Connection) {
        // TODO: Remove conflicting connections

        self.graph.add_edge(
            connection.source.dsp_id,
            connection.destination.dsp_id,
            connection,
        );

        self.mark_graph_needs_sort();
    }

    pub fn remove_connection(&mut self, connection: Connection) {
        self.graph
            .remove_edge(connection.source.dsp_id, connection.destination.dsp_id);

        self.mark_graph_needs_sort();
    }

    pub fn connect_to_output(&mut self, output_endpoint: Endpoint) {
        self.output_endpoint = Some(output_endpoint);
    }

    fn mix_in_endpoint(
        buffer_pool: &mut BufferPool,
        endpoint: Endpoint,
        output_buffer: &mut dyn AudioBuffer,
        num_channels: usize,
        num_frames: usize,
    ) {
        if let Some(buffer) = buffer_pool.get_assigned_buffer(endpoint) {
            let sample_location = SampleLocation::new(0, 0);
            output_buffer.add_from(
                &buffer,
                sample_location,
                sample_location,
                num_channels,
                num_frames,
            );

            buffer_pool.return_buffer_with_assignment(buffer, endpoint);
        }
    }

    fn write_to_output(
        &mut self,
        output_buffer: &mut dyn AudioBuffer,
        num_channels: usize,
        num_frames: usize,
    ) {
        if let Some(output_endpoint) = self.output_endpoint {
            Self::mix_in_endpoint(
                &mut self.buffer_pool,
                output_endpoint,
                output_buffer,
                num_channels,
                num_frames,
            );
        }
    }

    fn process_dsps(&mut self, num_frames: usize, num_channels: usize, start_time: &Timestamp) {
        for dsp_id in self.topological_sort.get_sorted_graph() {
            Self::process_dsp(
                &mut self.buffer_pool,
                &mut self.graph,
                *dsp_id,
                num_frames,
                num_channels,
                start_time,
            );
        }
    }

    fn copy_output_from_dependencies(
        buffer_pool: &mut BufferPool,
        graph: &Graph<Box<Dsp>, Connection>,
        dsp_id: Id,
        destination_buffer: &mut dyn AudioBuffer,
        num_channels: usize,
        num_frames: usize,
    ) {
        for connected_node_id in graph.node_iter(dsp_id, Direction::Incoming) {
            let endpoint = Endpoint::new(connected_node_id, EndpointType::Output);
            Self::mix_in_endpoint(
                buffer_pool,
                endpoint,
                destination_buffer,
                num_channels,
                num_frames,
            );
        }
    }

    fn process_dsp(
        buffer_pool: &mut BufferPool,
        graph: &mut Graph<Box<Dsp>, Connection>,
        dsp_id: Id,
        num_frames: usize,
        num_channels: usize,
        start_time: &Timestamp,
    ) {
        let output_endpoint = Endpoint::new(dsp_id, EndpointType::Output);

        let mut node_input_buffer = buffer_pool.get_unassigned_buffer().unwrap();
        let mut node_output_buffer = buffer_pool.get_unassigned_buffer().unwrap();

        let mut node_output_buffer_slice =
            AudioBufferSlice::new(&mut node_output_buffer, 0, num_frames);

        Self::copy_output_from_dependencies(
            buffer_pool,
            graph,
            dsp_id,
            &mut node_input_buffer,
            num_channels,
            num_frames,
        );

        if let Some(dsp) = graph.get_node_mut(dsp_id) {
            dsp.process_audio(
                &node_input_buffer,
                &mut node_output_buffer_slice,
                start_time,
            );
        };

        buffer_pool.return_buffer(node_input_buffer);
        buffer_pool.return_buffer_with_assignment(node_output_buffer, output_endpoint);
    }
}

#[cfg(test)]
mod tests {
    use approx::{assert_relative_eq, assert_relative_ne};

    use crate::{
        buffer::owned_audio_buffer::OwnedAudioBuffer,
        graph::dsp::{DspParameterMap, DspProcessor},
    };

    use super::*;

    struct Processor {
        value_to_write: f32,
        location_to_write: SampleLocation,
    }

    impl Processor {
        fn new(value_to_write: f32, location_to_write: SampleLocation) -> Self {
            Self {
                value_to_write,
                location_to_write,
            }
        }
    }

    impl DspProcessor for Processor {
        fn process_audio(
            &mut self,
            input_buffer: &dyn AudioBuffer,
            output_buffer: &mut dyn AudioBuffer,
            _start_time: &Timestamp,
            _parameters: &DspParameterMap,
        ) {
            output_buffer.add_from(
                input_buffer,
                SampleLocation::new(0, 0),
                SampleLocation::new(0, 0),
                output_buffer.num_channels(),
                output_buffer.num_frames(),
            );

            output_buffer.set_sample(self.location_to_write, self.value_to_write);
        }
    }

    fn make_dsp(value_to_write: f32, location_to_write: SampleLocation) -> Box<Dsp> {
        let processor = Box::new(Processor::new(value_to_write, location_to_write));
        let parameters = DspParameterMap::new();
        Box::new(Dsp::new(Id::generate(), processor, parameters))
    }

    #[test]
    fn renders_when_connected_to_output() {
        let value = 0.456;
        let location = SampleLocation::new(0, 27);

        let dsp = make_dsp(value, location);

        let dsp_id = dsp.get_id();
        let sample_rate = 44100;

        let mut graph = DspGraph::new(128, 2, sample_rate);
        graph.add_dsp(dsp);

        let num_frames = 128;

        let mut audio_buffer = OwnedAudioBuffer::new(num_frames, 2, sample_rate);
        graph.process(&mut audio_buffer, &Timestamp::default());

        assert_relative_ne!(audio_buffer.get_sample(location), value);

        graph.connect_to_output(Endpoint::new(dsp_id, EndpointType::Output));

        graph.process(&mut audio_buffer, &Timestamp::default());

        assert_relative_eq!(audio_buffer.get_sample(location), value);
    }

    #[test]
    fn renders_chain() {
        let value_1 = 0.123;
        let value_2 = 0.456;

        let location_1 = SampleLocation::new(0, 27);
        let location_2 = SampleLocation::new(1, 38);

        let dsp_1 = make_dsp(value_1, location_1);
        let dsp_2 = make_dsp(value_2, location_2);

        let dsp_id_1 = dsp_1.get_id();
        let dsp_id_2 = dsp_2.get_id();

        let sample_rate = 44100;

        let mut graph = DspGraph::new(128, 2, sample_rate);

        graph.add_dsp(dsp_1);
        graph.add_dsp(dsp_2);

        let num_frames = 128;

        graph.connect_to_output(Endpoint::new(dsp_id_2, EndpointType::Output));

        graph.add_connection(Connection::new(dsp_id_1, dsp_id_2));

        let mut audio_buffer = OwnedAudioBuffer::new(num_frames, 2, 44100);
        graph.process(&mut audio_buffer, &Timestamp::default());

        assert_relative_eq!(audio_buffer.get_sample(location_1), value_1);
        assert_relative_eq!(audio_buffer.get_sample(location_2), value_2);
    }

    #[test]
    fn doesnt_write_too_many_channels() {
        let dsp = make_dsp(0.0, SampleLocation::new(0, 0));
        let dsp_id = dsp.get_id();
        let sample_rate = 44100;
        let maximum_number_of_channels = 2;

        let mut graph = DspGraph::new(128, maximum_number_of_channels, sample_rate);

        graph.add_dsp(dsp);

        let num_frames = 128;

        graph.connect_to_output(Endpoint::new(dsp_id, EndpointType::Output));

        let mut audio_buffer =
            OwnedAudioBuffer::new(num_frames, maximum_number_of_channels * 2, 44100);

        graph.process(&mut audio_buffer, &Timestamp::default());
    }

    #[test]
    fn doesnt_write_too_many_frames() {
        let dsp = make_dsp(0.0, SampleLocation::new(0, 0));
        let dsp_id = dsp.get_id();
        let sample_rate = 44100;
        let maximum_number_of_frames = 512;

        let mut graph = DspGraph::new(maximum_number_of_frames, 2, sample_rate);

        graph.add_dsp(dsp);

        graph.connect_to_output(Endpoint::new(dsp_id, EndpointType::Output));

        let mut audio_buffer = OwnedAudioBuffer::new(maximum_number_of_frames * 2, 2, 44100);
        graph.process(&mut audio_buffer, &Timestamp::default());
    }
}
