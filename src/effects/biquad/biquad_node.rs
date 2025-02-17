use crate::{commands::Id, graph::DspParameters, AudioParameter, Context, GraphNode, Level};

use super::{biquad_processor::BiquadProcessor, filter_type::BiquadFilterType};

pub struct Biquad {
    pub node: GraphNode,
    pub frequency: AudioParameter,
    pub q: AudioParameter,
    pub shelf_gain: AudioParameter,
    pub gain: AudioParameter,
}

impl Biquad {
    pub fn new(context: &dyn Context, channel_count: usize, filter_type: BiquadFilterType) -> Self {
        let id = Id::generate();

        let (frequency, realtime_frequency) =
            AudioParameter::new(id, 1_000.0, 20.0, 20000.0, context.get_command_queue());

        let (q, realtime_q) = AudioParameter::new(
            id,
            1.0 / 2.0_f64.sqrt(),
            0.1,
            10.0,
            context.get_command_queue(),
        );

        let (shelf_gain, realtime_shelf_gain) = AudioParameter::new(
            id,
            Level::unity().as_gain(),
            0.0,
            Level::from_db(100.0).as_gain(),
            context.get_command_queue(),
        );

        let (gain, realtime_gain) = AudioParameter::new(
            id,
            Level::unity().as_gain(),
            0.0,
            Level::from_db(100.0).as_gain(),
            context.get_command_queue(),
        );

        let processor = Box::new(BiquadProcessor::new(
            context.get_sample_rate(),
            channel_count,
            filter_type,
            frequency.get_id(),
            q.get_id(),
            shelf_gain.get_id(),
            gain.get_id(),
        ));

        let node = GraphNode::new(
            id,
            context.get_command_queue(),
            channel_count,
            channel_count,
            processor,
            DspParameters::new([
                realtime_frequency,
                realtime_q,
                realtime_shelf_gain,
                realtime_gain,
            ]),
        );

        Self {
            node,
            frequency,
            q,
            shelf_gain,
            gain,
        }
    }
}
