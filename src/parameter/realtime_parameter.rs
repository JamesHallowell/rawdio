use crate::{commands::Id, Timestamp, MAXIMUM_FRAME_COUNT};

use super::{
    parameter_change::ValueChangeMethod, parameter_value::ParameterValue, ParameterChange,
};

use std::sync::atomic::Ordering;

const MAXIMUM_PENDING_PARAMETER_CHANGES: usize = 16;

#[repr(align(64))]
struct ParameterBuffer {
    values: Vec<f32>,
}

impl ParameterBuffer {
    fn reset(&mut self) {
        self.values.clear()
    }

    fn add_value(&mut self, value: f32) {
        self.values.push(value)
    }

    fn get_values(&self, frame_count: usize) -> &[f32] {
        &self.values[..frame_count]
    }
}

impl Default for ParameterBuffer {
    fn default() -> Self {
        Self {
            values: Vec::with_capacity(MAXIMUM_FRAME_COUNT),
        }
    }
}

pub struct RealtimeAudioParameter {
    parameter_id: Id,
    value: ParameterValue,
    parameter_changes: Vec<ParameterChange>,
    parameter_buffer: ParameterBuffer,
    increment: f64,
    coefficient: f64,
    current_change: ParameterChange,
}

impl RealtimeAudioParameter {
    pub fn new(parameter_id: Id, value: ParameterValue) -> Self {
        let initial_value = value.load(Ordering::Acquire);

        Self {
            parameter_id,
            value,
            parameter_changes: Vec::with_capacity(MAXIMUM_PENDING_PARAMETER_CHANGES),
            parameter_buffer: ParameterBuffer::default(),
            increment: 0.0,
            coefficient: 1.0,
            current_change: ParameterChange {
                value: initial_value,
                end_time: Timestamp::zero(),
                method: ValueChangeMethod::Immediate,
            },
        }
    }

    pub fn get_id(&self) -> Id {
        self.parameter_id
    }

    pub fn get_value(&self) -> f64 {
        self.value.load(Ordering::Acquire)
    }

    fn remove_expired_changes(&mut self, time: &Timestamp) {
        self.parameter_changes
            .retain(|param_change| param_change.end_time >= *time);
    }

    pub fn process(&mut self, time: &Timestamp, frame_count: usize, sample_rate: usize) {
        self.parameter_buffer.reset();

        self.remove_expired_changes(time);

        let mut value = self.get_value();
        for frame in 0..frame_count {
            let frame_time = time.incremented_by_samples(frame, sample_rate);

            value = self.process_change(&frame_time, sample_rate, value);

            self.parameter_buffer.add_value(value as f32);
        }

        self.set_value(value);
    }

    fn process_change(&mut self, time: &Timestamp, sample_rate: usize, value: f64) -> f64 {
        if let Some(next_event) = self.parameter_changes.first() {
            match next_event.method {
                ValueChangeMethod::Immediate => {
                    if next_event.end_time <= *time {
                        self.increment = 0.0;
                        self.coefficient = 1.0;
                        self.current_change = self.parameter_changes.remove(0);
                    }
                }
                ValueChangeMethod::Linear(start_time) => {
                    if *time >= start_time {
                        let duration = next_event.end_time.as_samples(sample_rate)
                            - time.as_samples(sample_rate);
                        let delta = next_event.value - value;

                        self.increment = delta / duration;
                        self.coefficient = 1.0;
                        self.current_change = self.parameter_changes.remove(0);
                    }
                }
                ValueChangeMethod::Exponential(start_time) => {
                    if *time >= start_time {
                        debug_assert_ne!(next_event.value, 0.0);
                        debug_assert_ne!(value, 0.0);

                        let ratio = next_event.value / value;

                        let duration = next_event.end_time.as_samples(sample_rate)
                            - time.as_samples(sample_rate);

                        self.increment = 0.0;
                        self.coefficient = (ratio.ln() / duration).exp();

                        self.current_change = self.parameter_changes.remove(0);
                    }
                }
            };
        }

        if self.current_change.end_time <= *time {
            return self.current_change.value;
        }

        (value * self.coefficient) + self.increment
    }

    pub fn get_values(&self, frame_count: usize) -> &[f32] {
        self.parameter_buffer.get_values(frame_count)
    }

    pub fn set_value(&mut self, value: f64) {
        self.value.store(value, Ordering::Release)
    }

    pub fn add_parameter_change(&mut self, parameter_change: ParameterChange) {
        self.parameter_changes.push(parameter_change);

        self.parameter_changes
            .sort_by(|a, b| a.end_time.partial_cmp(&b.end_time).unwrap());
    }

    pub fn cancel_scheduled_changes_ending_after(&mut self, time: &Timestamp) {
        self.parameter_changes
            .retain(|change| change.end_time >= *time);
    }

    pub fn cancel_scheduled_changes(&mut self) {
        self.parameter_changes.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use atomic_float::AtomicF64;

    fn process_parameter_values(
        parameter: &mut RealtimeAudioParameter,
        from_time: Timestamp,
        to_time: Timestamp,
        sample_rate: usize,
    ) -> Vec<f32> {
        let mut values = Vec::new();

        let start_sample = from_time.as_samples(sample_rate).ceil() as usize;
        let end_sample = to_time.as_samples(sample_rate).ceil() as usize;

        for frame in (start_sample..end_sample).step_by(MAXIMUM_FRAME_COUNT) {
            let frame_end_sample = (frame + MAXIMUM_FRAME_COUNT).min(end_sample);
            let current_time = from_time.incremented_by_samples(frame, sample_rate);
            let frame_count = frame_end_sample - frame;

            parameter.process(&current_time, frame_count, sample_rate);

            values.extend_from_slice(parameter.get_values(frame_count));
        }

        values
    }

    #[test]
    fn immediate_parameter_changes() {
        let id = Id::generate();
        let value = ParameterValue::new(AtomicF64::new(0.0));
        let mut param = RealtimeAudioParameter::new(id, value);

        param.add_parameter_change(ParameterChange {
            value: 1.0,
            end_time: Timestamp::from_seconds(1.0),
            method: ValueChangeMethod::Immediate,
        });

        param.add_parameter_change(ParameterChange {
            value: 2.0,
            end_time: Timestamp::from_seconds(2.0),
            method: ValueChangeMethod::Immediate,
        });

        param.add_parameter_change(ParameterChange {
            value: 3.0,
            end_time: Timestamp::from_seconds(3.0),
            method: ValueChangeMethod::Immediate,
        });

        let sample_rate = 48_000;
        let max_time = 3.5;
        let values = process_parameter_values(
            &mut param,
            Timestamp::zero(),
            Timestamp::from_seconds(max_time),
            sample_rate,
        );

        let get_value_at_time = |time: f64| {
            let offset = Timestamp::from_seconds(time).as_samples(sample_rate).ceil() as usize;
            assert!(offset < values.len());
            values[offset]
        };

        assert_relative_eq!(get_value_at_time(0.9), 0.0);
        assert_relative_eq!(get_value_at_time(1.0), 1.0);
        assert_relative_eq!(get_value_at_time(1.9), 1.0);
        assert_relative_eq!(get_value_at_time(2.0), 2.0);
        assert_relative_eq!(get_value_at_time(2.9), 2.0);
        assert_relative_eq!(get_value_at_time(3.0), 3.0);
    }

    #[test]
    fn ramped_parameter_changes() {
        let id = Id::generate();
        let value = ParameterValue::new(AtomicF64::new(0.0));
        let mut param = RealtimeAudioParameter::new(id, value);

        [
            (1.0, Timestamp::zero(), Timestamp::from_seconds(1.0)),
            (
                2.0,
                Timestamp::from_seconds(1.0),
                Timestamp::from_seconds(2.0),
            ),
            (
                3.0,
                Timestamp::from_seconds(2.0),
                Timestamp::from_seconds(3.0),
            ),
        ]
        .iter()
        .for_each(|(value, start_time, end_time)| {
            param.add_parameter_change(ParameterChange {
                value: *value,
                end_time: *end_time,
                method: ValueChangeMethod::Linear(*start_time),
            });
        });

        let sample_rate = 48_000;
        let values = process_parameter_values(
            &mut param,
            Timestamp::zero(),
            Timestamp::from_seconds(3.5),
            sample_rate,
        );

        let get_value_at_time = |time: f64| {
            let offset = Timestamp::from_seconds(time).as_samples(sample_rate).ceil() as usize;
            values[offset]
        };

        assert_relative_eq!(get_value_at_time(0.5), 0.5, epsilon = 1e-3);
        assert_relative_eq!(get_value_at_time(1.0), 1.0, epsilon = 1e-3);
        assert_relative_eq!(get_value_at_time(1.5), 1.5, epsilon = 1e-3);
        assert_relative_eq!(get_value_at_time(2.0), 2.0, epsilon = 1e-3);
        assert_relative_eq!(get_value_at_time(2.5), 2.5, epsilon = 1e-3);
        assert_relative_eq!(get_value_at_time(3.0), 3.0, epsilon = 1e-3);
    }

    #[test]
    fn exponential_ramps() {
        let id = Id::generate();
        let initial_value = 2.0;
        let value = ParameterValue::new(AtomicF64::new(initial_value));
        let mut param = RealtimeAudioParameter::new(id, value);

        let ramp_duration = Timestamp::from_seconds(1.0);

        param.add_parameter_change(ParameterChange {
            value: 2.0 * initial_value,
            end_time: ramp_duration,
            method: ValueChangeMethod::Exponential(Timestamp::zero()),
        });

        let sample_rate = 96_000;

        let values = process_parameter_values(
            &mut param,
            Timestamp::zero(),
            ramp_duration.incremented_by_seconds(0.1),
            sample_rate,
        );

        let get_value_at_time = |time: f64| {
            let offset = Timestamp::from_seconds(time).as_samples(sample_rate).ceil() as usize;
            values[offset]
        };

        assert_relative_eq!(get_value_at_time(0.0), 2.0, epsilon = 1e-3);
        assert_relative_eq!(get_value_at_time(0.5), 2.0 * 1.414, epsilon = 1e-3);
        assert_relative_eq!(get_value_at_time(1.0), 4.0, epsilon = 1e-3);
    }

    #[test]
    fn smooth_ramp() {}
}
