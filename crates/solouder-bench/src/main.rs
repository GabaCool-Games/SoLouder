//! Times `Renderer::render` for every case and buffer size and prints one
//! tab-separated line per run (see `scripts/run_bench.py`).
//!
//! ```text
//! solouder-bench [--seconds S] [--case SUBSTRING] [--buffers 128,256,...]
//! ```
//!
//! Build with `--release`; a debug build prints a warning.

use solouder_bench::cases::{self, Reference};
use solouder_bench::session::{micros, renders_for};
use solouder_bench::signal::OUTPUT_RATE;
use solouder_bench::timing::budget;

/// Buffer sizes, in frames, every case runs at.
const BUFFERS: [u32; 5] = [128, 256, 480, 512, 1024];

/// Untimed output before timing starts, in seconds.
const WARMUP_SECONDS: f64 = 0.25;

struct Options {
    seconds: f64,
    filter: Option<String>,
    buffers: Vec<u32>,
}

fn parse() -> Result<Options, String> {
    let mut options = Options {
        seconds: 2.0,
        filter: None,
        buffers: BUFFERS.to_vec(),
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--seconds" => {
                options.seconds = value()?.parse().map_err(|e| format!("--seconds: {e}"))?;
            }
            "--case" => options.filter = Some(value()?),
            "--buffers" => {
                options.buffers = value()?
                    .split(',')
                    .map(|b| b.trim().parse().map_err(|e| format!("--buffers: {e}")))
                    .collect::<Result<_, _>>()?;
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    let positive = options.seconds.is_finite() && options.seconds > 0.0;
    if !positive || options.buffers.contains(&0) {
        return Err("seconds and buffer sizes must be positive".to_owned());
    }
    Ok(options)
}

fn main() {
    let options = match parse() {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    if cfg!(debug_assertions) {
        eprintln!("warning: a debug build; run with --release for real numbers");
    }
    println!("case\tbuffer\trenders\tmean_us\tp99_us\tmax_us\tbudget_us\tactive\tnote");
    for case in cases::all() {
        if let Some(filter) = &options.filter
            && !case.name.contains(filter.as_str())
        {
            continue;
        }
        for &buffer in &options.buffers {
            let mut session = (case.build)(buffer);
            let warmup = renders_for(WARMUP_SECONDS, buffer).max(8);
            let timing = session.run(buffer, warmup, renders_for(options.seconds, buffer));
            let status = session.status();
            session.finish();
            let note = match case.reference {
                Reference::Same => String::new(),
                Reference::DecodesInMix => {
                    format!("paced; underflow {} frames", status.stream_underflow_frames)
                }
                Reference::None => "no C++ counterpart".to_owned(),
            };
            println!(
                "{}\t{buffer}\t{}\t{:.1}\t{:.1}\t{:.1}\t{:.1}\t{}\t{note}",
                case.name,
                timing.renders,
                micros(timing.mean),
                micros(timing.p99),
                micros(timing.max),
                micros(budget(buffer, OUTPUT_RATE)),
                status.active_voices,
            );
        }
    }
}
