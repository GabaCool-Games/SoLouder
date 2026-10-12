//! Plays a WAV file (or a generated chord) on the default output device.
//!
//! ```sh
//! cargo run -p solouder-cpal --example play_wav -- [path/to/file.wav]
//! ```

use std::error::Error;
use std::thread::sleep;
use std::time::{Duration, Instant};

use solouder_core::{Event, Pcm, Sound};
use solouder_cpal::{OutputConfig, OutputState, open};
use solouder_decoders::{DecodeLimits, decode_wav};

fn chord() -> Result<Pcm, Box<dyn Error>> {
    let rate = 44_100.0_f32;
    let samples = (0..(rate as usize * 2))
        .map(|i| {
            let t = i as f32 / rate;
            let fade = (1.0 - t / 2.0).max(0.0);
            [220.0_f32, 277.18, 329.63]
                .iter()
                .map(|f| (std::f32::consts::TAU * f * t).sin())
                .sum::<f32>()
                * 0.2
                * fade
        })
        .collect();
    Ok(Pcm::from_planar(samples, 1, rate)?)
}

fn main() -> Result<(), Box<dyn Error>> {
    let pcm = match std::env::args().nth(1) {
        Some(path) => decode_wav(&std::fs::read(&path)?, &DecodeLimits::default())?,
        None => chord()?,
    };
    let seconds = f64::from(pcm.frames()) / f64::from(pcm.sample_rate());
    let sound = Sound::new(pcm);

    let (output, mut controller) = open(&OutputConfig::default())?;
    println!("output: {:?}", output.info());
    let id = controller.play(&sound)?;
    println!("playing {id:?} ({seconds:.2} s)");

    let deadline = Instant::now() + Duration::from_secs_f64(seconds + 2.0);
    'wait: while Instant::now() < deadline {
        while let Some(event) = controller.poll_event() {
            println!("event: {event:?}");
            if matches!(event, Event::Finished(..) | Event::Failed(..)) {
                break 'wait;
            }
        }
        if output.state() != OutputState::Playing {
            println!("output stopped: {:?}", output.state());
            break;
        }
        sleep(Duration::from_millis(20));
    }

    let status = controller.status();
    println!(
        "renders {}, callbacks {}, backend errors {}, stream time {:.2} s",
        status.renders,
        output.callback_count(),
        output.error_count(),
        status.stream_time
    );
    let renderer = output.close();
    println!("renderer returned on close: {}", renderer.is_some());
    Ok(())
}
