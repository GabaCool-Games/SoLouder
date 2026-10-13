//! Scenario operations on audio files: `load` and `samples`. Each mirrors the
//! operation of the same name in `reference/tests/parity_runner.cpp`.

use std::path::PathBuf;

use solouder_core::Sound;
use solouder_decoders::{DecodeError, DecodeLimits, decode_wav};

use crate::ops::hex32;
use crate::run::{SO_NO_ERROR, State};
use crate::scenario::{ScenarioError, Step};

/// SoLoud's `FILE_LOAD_FAILED`.
pub(crate) const FILE_LOAD_FAILED: i32 = 3;
/// SoLoud's `OUT_OF_MEMORY`.
const OUT_OF_MEMORY: i32 = 5;

/// `tests/fixtures/`, which `file=` paths are relative to.
fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

/// The fixture named by `file=`: letters, digits, `.`, `_`, `-` and `/`
/// separators, without `..` or a leading `/`.
pub(crate) fn fixture_path(step: &Step) -> Result<PathBuf, ScenarioError> {
    let file = step.text("file")?;
    let allowed = |c: char| c.is_ascii_alphanumeric() || "._-/".contains(c);
    if file.is_empty()
        || file.starts_with('/')
        || !file.chars().all(allowed)
        || file.split('/').any(|part| part.is_empty() || part == "..")
    {
        return Err(step.error(format!("invalid fixture path `{file}`")));
    }
    Ok(fixture_dir().join(file))
}

pub(crate) fn load_code(error: &DecodeError) -> i32 {
    match error {
        DecodeError::OutOfMemory => OUT_OF_MEMORY,
        _ => FILE_LOAD_FAILED,
    }
}

impl State {
    pub(crate) fn load(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&["name", "file", "volume", "looping"])?;
        self.mixer(step)?;
        let name = step.text("name")?.to_owned();
        let path = fixture_path(step)?;
        let volume = step.number_or("volume", 1.0)?;
        let looping = step.flag_or("looping", false)?;
        let bytes = std::fs::read(&path)
            .map_err(|error| step.error(format!("cannot read {}: {error}", path.display())))?;

        let code = match decode_wav(&bytes, &DecodeLimits::default()) {
            Ok(pcm) => {
                let mut sound = Sound::new(pcm);
                sound.set_volume(volume);
                sound.set_looping(looping);
                self.store(name.clone(), sound);
                SO_NO_ERROR
            }
            Err(error) => load_code(&error),
        };
        self.log(format!("load {name} {code}"));
        Ok(())
    }

    pub(crate) fn samples(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&["name"])?;
        let name = step.text("name")?;
        let sound = self
            .sounds
            .get(name)
            .ok_or_else(|| step.error(format!("no sound named `{name}`")))?;
        let pcm = sound.pcm();
        let line = format!(
            "samples {name} {} {} {}",
            pcm.channels(),
            pcm.frames(),
            hex32(pcm.sample_rate())
        );
        for channel in 0..pcm.channels() {
            self.rendered
                .samples
                .extend_from_slice(pcm.channel(channel));
        }
        self.log(line);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{parse, run};

    fn run_text(text: &str) -> Result<crate::Rendered, crate::ScenarioError> {
        run(&parse("test", text).unwrap())
    }

    #[test]
    fn loads_and_dumps_a_fixture() {
        let rendered =
            run_text("init\nload name=a file=wav/s16_stereo_44100.wav\nsamples name=a\n").unwrap();
        assert_eq!(rendered.log[1], "load a 0");
        assert_eq!(rendered.log[2], "samples a 2 1500 472c4400");
        assert_eq!(rendered.samples.len(), 3000);
        assert_eq!(rendered.samples[0], -1.0);
    }

    #[test]
    fn reports_load_failures_with_soloud_codes() {
        let rendered = run_text("init\nload name=a file=wav/bad_nine_channels.wav\n").unwrap();
        assert_eq!(rendered.log[1], "load a 3");
    }

    #[test]
    fn rejects_paths_outside_the_fixtures() {
        for file in [
            "../x.wav",
            "/x.wav",
            "wav//x.wav",
            "wav/x*y.wav",
            "wav\\x.wav",
        ] {
            let text = format!("init\nload name=a file={file}\n");
            assert!(run_text(&text).is_err(), "{file}");
        }
        assert!(run_text("init\nload name=a file=wav/missing.wav\n").is_err());
        assert!(run_text("init\nsamples name=nothing\n").is_err());
    }
}
