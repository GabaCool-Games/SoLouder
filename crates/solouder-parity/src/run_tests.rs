use super::*;
use crate::scenario::parse;

fn run_text(text: &str) -> Result<Rendered, ScenarioError> {
    run(&parse("test", text).unwrap())
}

#[test]
fn logs_init_results_and_mixes_requested_frames() {
    let rendered = run_text("init channels=3\ninit channels=6\nmix frames=5").unwrap();
    assert_eq!(rendered.log, ["init 1", "init 0"]);
    assert_eq!(rendered.samples, vec![0.0; 30]);
}

#[test]
fn failed_init_drops_the_previous_engine() {
    let error = run_text("init\ninit buffer=100\nmix frames=1").unwrap_err();
    assert_eq!(error.line, 3);
}

#[test]
fn rejects_unknown_operations_and_oversized_mixes() {
    assert!(run_text("fade h=x").is_err());
    assert!(run_text("init\nmix frames=4097").is_err());
    assert!(run_text("init volume=1").is_err());
    assert!(run_text("init clip=soft").is_err());
}

#[test]
fn tolerance_sets_the_comparison_limit() {
    assert_eq!(run_text("tolerance abs=0.5").unwrap().tolerance, 0.5);
    assert!(run_text("tolerance abs=-1").is_err());
}

#[test]
fn plays_and_queries_a_sound() {
    let text = "init\n\
                sound name=s frames=1024 pattern=dc amp=0.5\n\
                play sound=s as=a pan=0.5\n\
                set h=a speed=0\n\
                mix frames=16\n\
                query h=a\n\
                counts\n\
                stop h=a\n\
                query h=a";
    let rendered = run_text(text).unwrap();
    assert_eq!(
        rendered.log[..4],
        ["init 0", "sound s 0", "play a 1", "set a speed 1"]
    );
    assert!(rendered.log[4].starts_with("query a valid=1 paused=0 looping=0 protect=0 loops=0"));
    assert_eq!(rendered.log[5], "counts voices=1 active=1");
    assert!(rendered.log[6].starts_with("query a valid=0"));
    assert!(rendered.samples.iter().skip(2).all(|&s| s > 0.0));
}

#[test]
fn invalid_sounds_are_logged_and_not_registered() {
    let rendered = run_text("init\nsound name=s channels=9 frames=4").unwrap();
    assert_eq!(rendered.log[1], "sound s 1");
    assert!(run_text("init\nsound name=s channels=9 frames=4\nplay sound=s as=a").is_err());
}

#[test]
fn operations_need_init_and_known_names() {
    assert!(run_text("sound name=s frames=4").is_err());
    assert!(run_text("init\nstop h=missing").is_err());
    assert!(run_text("init\nplay sound=missing as=a").is_err());
    assert!(run_text("init\nsound name=s frames=4\ninit\nplay sound=s as=a").is_err());
    assert!(run_text("init\nglobal resampler=cubic").is_err());
}
