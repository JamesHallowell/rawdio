use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Host, Stream,
};
use rust_audio_engine::realtime_context::RealtimeContext;

const SAMPLE_RATE: u32 = 44100;

pub struct AudioCallback {
    _output_stream: Stream,
}

fn print_output_devices(host: &Host) {
    println!("Output devices: ");
    host.output_devices().unwrap().for_each(|device| {
        let device_name = match device.name() {
            Ok(name) => name,
            Err(_) => return,
        };

        println!("{}", device_name);
    });
    println!();
}

impl AudioCallback {
    pub fn new(mut realtime_context: Box<dyn RealtimeContext + Send>) -> Self {
        let host = cpal::default_host();
        println!("Using audio host: {}\n", host.id().name());

        print_output_devices(&host);

        let preferred_device = host.default_output_device();

        let device = preferred_device.expect("Couldn't connect to output audio device");

        let mut output_configs = device.supported_output_configs().unwrap();
        let config = output_configs
            .next()
            .expect("No configs supported")
            .with_sample_rate(cpal::SampleRate(SAMPLE_RATE));

        println!("Connecting to device: {}", device.name().unwrap());
        println!("Sample rate: {}\n", config.sample_rate().0);

        let stream = device
            .build_output_stream(
                &config.config(),
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    realtime_context.process(data, config.channels().into());
                },
                move |err| eprintln!("Stream error: {:?}", err),
            )
            .expect("Couldn't create output stream");

        stream.play().expect("Couldn't start output stream");

        Self {
            _output_stream: stream,
        }
    }
}
