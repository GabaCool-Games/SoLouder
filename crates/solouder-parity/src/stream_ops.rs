//! The `stream` scenario operation, which mirrors the operation of the same
//! name in `reference/tests/parity_runner.cpp`.

use std::io::Cursor;
use std::sync::Arc;

use solouder_core::Stream;
use solouder_decoders::{StreamFile, StreamInput, WavStream};

use crate::files::{FILE_LOAD_FAILED, fixture_path, load_code, open_fixture, read_fixture};
use crate::run::{SO_NO_ERROR, State};
use crate::scenario::{ScenarioError, Step};

/// A caller's own file type over bytes, as a SoLoud `MemoryFile` passed to
/// `WavStream::loadFile`.
#[derive(Debug)]
struct MemoryFile(Arc<[u8]>);

impl StreamFile for MemoryFile {
    fn open(&self) -> std::io::Result<Box<dyn StreamInput>> {
        Ok(Box::new(Cursor::new(Arc::clone(&self.0))))
    }
}

impl State {
    /// `WavStream::load`, `loadToMem` (`from=memory`), `loadMem` of the
    /// file's bytes (`from=bytes`), `loadFile` of a `DiskFile` or
    /// `MemoryFile` (`from=disk_file`, `memory_file`) or `loadFileToMem` of
    /// one (`from=disk_to_mem`, `memory_to_mem`), then the stream's
    /// settings.
    pub(crate) fn stream(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&["name", "file", "from", "volume", "looping", "loop_point"])?;
        self.mixer(step)?;
        let name = step.text("name")?.to_owned();
        let path = fixture_path(step)?;
        let loaded = match step.get("from").unwrap_or("file") {
            "file" => WavStream::open(&path),
            "memory" => WavStream::load_to_memory(&path),
            "bytes" => WavStream::from_bytes(read_fixture(step, &path)?),
            // `loadFile` of a `DiskFile` (each voice opens the path again)
            // or of a `MemoryFile` (bytes held by the caller's own file).
            "disk_file" => {
                open_fixture(step, &path)?;
                WavStream::from_file(path.clone())
            }
            "memory_file" => WavStream::from_file(MemoryFile(read_fixture(step, &path)?.into())),
            "disk_to_mem" => WavStream::from_reader(open_fixture(step, &path)?),
            "memory_to_mem" => WavStream::from_reader(Cursor::new(read_fixture(step, &path)?)),
            other => return Err(step.error(format!("unknown from `{other}`"))),
        };
        let volume = step.number_or("volume", 1.0)?;
        let looping = step.flag_or("looping", false)?;
        let loop_point = step.seconds_or("loop_point", 0.0)?;
        // Reloading a name stops the old stream's voices, whether or not
        // the new file loads (patch 0063); a failed reload forgets the name.
        if let Some(old) = self.streams.remove(&name) {
            self.mixer(step)?.stop_sound(&old);
        }
        let code = match loaded.map(|wav| Stream::new(Arc::new(wav))) {
            Ok(Ok(mut stream)) => {
                stream.set_volume(volume);
                stream.set_looping(looping);
                stream.set_loop_point(loop_point);
                self.streams.insert(name.clone(), stream);
                SO_NO_ERROR
            }
            Ok(Err(_)) => FILE_LOAD_FAILED,
            Err(error) => load_code(&error),
        };
        self.log(format!("stream {name} {code}"));
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
    fn streams_load_from_files_memory_and_bytes() {
        let text = "init\nstream name=a file=wav/s16_stereo_44100.wav\n\
                    stream name=b file=wav/s16_stereo_44100.wav from=memory\n\
                    stream name=c file=wav/s16_stereo_44100.wav from=bytes\n\
                    stream name=d file=wav/bad_nine_channels.wav\n\
                    play sound=a as=v\nsound_query name=a\n";
        let rendered = run_text(text).unwrap();
        assert_eq!(
            rendered.log[1..5],
            ["stream a 0", "stream b 0", "stream c 0", "stream d 3"]
        );
        assert!(rendered.log[6].ends_with("count=1"));
        assert!(run_text("init\nstream name=a file=wav/s16_stereo_44100.wav from=x\n").is_err());
    }
}
