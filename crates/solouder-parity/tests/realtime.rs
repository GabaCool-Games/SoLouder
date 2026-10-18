//! The real-time controller and renderer render the spatial scenarios as the
//! mixer does: each is replayed through them (`replay_realtime`) and its
//! samples compared bit for bit with `run`'s. The mixer's run is compared
//! with the C++ reference by `parity.rs`, so this ties the real-time path to
//! the reference too.

use std::path::Path;

use solouder_parity::{parse, replay_realtime, run};

/// Each spatial scenario, and the line and operation its replay stops at,
/// if any. Editing a scenario above its stop moves the line: update it here.
const SCENARIOS: &[(&str, Option<(usize, &str)>)] = &[
    ("spatial_3d", Some((260, "`global`"))),
    ("spatial_custom", Some((204, "`sound3d`"))),
    ("spatial_delay", None),
    ("spatial_doppler", None),
    ("spatial_sources", None),
    ("spatial_speakers", None),
];

#[test]
fn spatial_scenarios_render_in_real_time_as_on_the_mixer() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/scenarios");
    let mut failures = Vec::new();
    for &(name, expected_stop) in SCENARIOS {
        let text = std::fs::read_to_string(dir.join(format!("{name}.scn"))).unwrap();
        let scenario = parse(name, &text).unwrap();
        let offline = run(&scenario).unwrap();
        let replayed = replay_realtime(&scenario).unwrap();
        let stopped = replayed.stopped.as_ref();
        println!(
            "{name}: {} of {} samples replayed; stopped at {stopped:?}",
            replayed.samples.len(),
            offline.samples.len()
        );
        match (stopped, expected_stop) {
            (None, None) => {}
            (Some((line, reason)), Some((at, op))) if *line == at && reason.starts_with(op) => {}
            _ => failures.push(format!(
                "{name}: stopped at {stopped:?}, not {expected_stop:?}"
            )),
        }
        let len = replayed.samples.len();
        if expected_stop.is_none() && len != offline.samples.len() {
            failures.push(format!("{name}: replayed {len} samples"));
        }
        // Most of the scenario must replay for the comparison to count.
        if len * 2 < offline.samples.len() {
            failures.push(format!("{name}: only {len} samples replayed"));
        }
        let differing = offline.samples[..len.min(offline.samples.len())]
            .iter()
            .zip(&replayed.samples)
            .position(|(a, b)| a.to_bits() != b.to_bits());
        if let Some(index) = differing {
            failures.push(format!(
                "{name}: sample {index} differs: mixer {}, real time {}",
                offline.samples[index], replayed.samples[index]
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
