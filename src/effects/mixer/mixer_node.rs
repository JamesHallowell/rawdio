use crate::{commands::Id, effects::Channel, graph::DspParameters, CommandQueue, GraphNode, Level};

use super::{
    mixer_event::EventTransmitter, mixer_matrix::MixerMatrix, mixer_processor::MixerProcessor,
};

pub struct Mixer {
    pub node: GraphNode,
    pub gain_matrix: MixerMatrix,
    event_transmitter: EventTransmitter,
}

static EVENT_CHANNEL_CAPACITY: usize = 32;

impl Mixer {
    pub fn new(
        command_queue: Box<dyn CommandQueue>,
        input_count: usize,
        output_count: usize,
    ) -> Self {
        let id = Id::generate();

        let gain_matrix = MixerMatrix::new(input_count, output_count);

        let (event_transmitter, event_receiver) = Channel::bounded(EVENT_CHANNEL_CAPACITY);

        let processor = Box::new(MixerProcessor::new(event_receiver));

        let node = GraphNode::new(
            id,
            command_queue,
            input_count,
            output_count,
            processor,
            DspParameters::empty(),
        );

        Self {
            node,
            gain_matrix,
            event_transmitter,
        }
    }

    pub fn set_level(&mut self, input_channel: usize, output_channel: usize, level: Level) {
        self.gain_matrix
            .set_level(input_channel, output_channel, level);

        let _ = self.event_transmitter.send(self.gain_matrix.clone());
    }

    pub fn mono_to_stereo_splitter(command_queue: Box<dyn CommandQueue>) -> Self {
        let input_count = 1;
        let output_count = 2;

        let mut mixer = Self::new(command_queue, input_count, output_count);

        mixer.set_level(0, 0, Level::unity());
        mixer.set_level(0, 1, Level::unity());

        mixer
    }
}
