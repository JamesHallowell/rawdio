use std::cell::RefCell;

use crate::{
    buffer::audio_buffer::AudioBuffer,
    commands::{command::ParameterChangeRequest, id::Id},
    graph::{connection::Connection, dsp::Dsp, endpoint::Endpoint},
    timestamp::Timestamp,
    utility::garbage_collector::{run_garbage_collector, GarbageCollectionCommand},
};

use lockfree::channel::{spsc, spsc::Sender};

use super::{graph::Graph, topological_sort::TopologicalSort};

pub struct DspGraph {
    graph: Graph<RefCell<Dsp>, Connection>,
    topological_sort: TopologicalSort,
    output_endpoint: Option<Endpoint>,
    garbase_collection_tx: Sender<GarbageCollectionCommand>,
    graph_needs_sort: bool,
}

impl DspGraph {
    pub fn process(&mut self, output_buffer: &mut dyn AudioBuffer, start_time: &Timestamp) {
        output_buffer.clear();

        if self.graph_needs_sort {
            self.topological_sort.sort(&self.graph);
            self.graph_needs_sort = false;
        }

        for dsp_id in self.topological_sort.get_sorted_graph() {
            self.process_dsp(*dsp_id, output_buffer, start_time);
        }
    }

    pub fn add_dsp(&mut self, dsp: RefCell<Dsp>) {
        let id = dsp.borrow().get_id();
        self.graph.add_node_with_id(id, dsp);
        self.mark_graph_needs_sort();
    }

    pub fn mark_graph_needs_sort(&mut self) {
        self.graph_needs_sort = true;
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
            dsp.borrow_mut().request_parameter_change(change_request);
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

    fn process_dsp(&self, dsp_id: Id, output_buffer: &mut dyn AudioBuffer, start_time: &Timestamp) {
        let dsp = match self.graph.get_node(dsp_id) {
            Some(node) => node,
            None => return,
        };

        dsp.borrow_mut().process_audio(output_buffer, start_time);
    }
}

impl Default for DspGraph {
    fn default() -> Self {
        let (garbase_collection_tx, garbage_collection_rx) = spsc::create();
        run_garbage_collector(garbage_collection_rx);

        Self {
            graph: Graph::with_capacity(512, 512),
            topological_sort: TopologicalSort::with_capacity(512),
            graph_needs_sort: false,
            output_endpoint: None,
            garbase_collection_tx,
        }
    }
}

#[cfg(test)]
mod tests {

    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    use crate::{
        buffer::owned_audio_buffer::OwnedAudioBuffer,
        graph::dsp::{DspParameterMap, DspProcessor},
    };

    use super::*;

    struct Processor {
        frame_count: Arc<AtomicUsize>,
    }

    impl Processor {
        fn new(frame_count: Arc<AtomicUsize>) -> Self {
            Self { frame_count }
        }
    }

    impl DspProcessor for Processor {
        fn process_audio(
            &mut self,
            output_buffer: &mut dyn AudioBuffer,
            _start_time: &Timestamp,
            _parameters: &DspParameterMap,
        ) {
            self.frame_count.fetch_add(
                output_buffer.num_frames(),
                std::sync::atomic::Ordering::SeqCst,
            );
        }
    }

    fn make_dsp(frame_count: Arc<AtomicUsize>) -> RefCell<Dsp> {
        let processor = Box::new(Processor::new(frame_count));
        let parameters = DspParameterMap::new();
        RefCell::new(Dsp::new(Id::generate(), processor, parameters))
    }

    #[test]
    fn renders_when_connected_to_output() {
        let frame_count = Arc::new(AtomicUsize::new(0));
        let dsp = make_dsp(frame_count.clone());
        let dsp_id = dsp.borrow().get_id();
        let mut graph = DspGraph::default();
        graph.add_dsp(dsp);

        let num_frames = 128;

        let mut audio_buffer = OwnedAudioBuffer::new(num_frames, 2, 44100);
        graph.process(&mut audio_buffer, &Timestamp::default());

        assert_eq!(frame_count.load(Ordering::Acquire), 0);

        graph.connect_to_output(Endpoint::new(dsp_id));

        graph.process(&mut audio_buffer, &Timestamp::default());

        assert_eq!(frame_count.load(Ordering::Acquire), num_frames);
    }

    #[test]
    fn renders_chain() {
        let frame_count_1 = Arc::new(AtomicUsize::new(0));
        let frame_count_2 = Arc::new(AtomicUsize::new(0));

        let dsp_1 = make_dsp(frame_count_1.clone());
        let dsp_2 = make_dsp(frame_count_2.clone());

        let dsp_id_1 = dsp_1.borrow().get_id();
        let dsp_id_2 = dsp_2.borrow().get_id();

        let mut graph = DspGraph::default();

        graph.add_dsp(dsp_1);
        graph.add_dsp(dsp_2);

        let num_frames = 128;

        graph.connect_to_output(Endpoint::new(dsp_id_2));

        graph.add_connection(Connection::new(dsp_id_1, dsp_id_2));

        let mut audio_buffer = OwnedAudioBuffer::new(num_frames, 2, 44100);
        graph.process(&mut audio_buffer, &Timestamp::default());

        assert_eq!(frame_count_1.load(Ordering::Acquire), num_frames);
        assert_eq!(frame_count_2.load(Ordering::Acquire), num_frames);
    }
}
