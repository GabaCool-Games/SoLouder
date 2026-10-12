//! Loads C++ reference output and compares it with a Rust run.

use std::io;
use std::path::Path;

use crate::run::Rendered;

/// Output of the C++ runner for one scenario.
#[derive(Clone, Debug, PartialEq)]
pub struct Golden {
    /// Interleaved samples from `<name>.f32`.
    pub samples: Vec<f32>,
    /// Lines of `<name>.log`.
    pub log: Vec<String>,
}

/// Sample differences of a successful comparison.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stats {
    /// Number of samples compared.
    pub samples: usize,
    /// Largest absolute difference.
    pub max_abs: f32,
    /// Root-mean-square difference.
    pub rms: f64,
}

/// Reads `<dir>/<name>.f32` and `<dir>/<name>.log`. Returns `None` when the
/// sample file does not exist.
pub fn load_golden(dir: &Path, name: &str) -> io::Result<Option<Golden>> {
    let samples_path = dir.join(format!("{name}.f32"));
    let bytes = match std::fs::read(&samples_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if bytes.len() % 4 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{} is not a whole number of f32 samples",
                samples_path.display()
            ),
        ));
    }
    let (chunks, _) = bytes.as_chunks::<4>();
    let samples = chunks
        .iter()
        .map(|chunk| f32::from_le_bytes(*chunk))
        .collect();
    let log_text = std::fs::read_to_string(dir.join(format!("{name}.log")))?;
    let log = log_text.lines().map(str::to_owned).collect();
    Ok(Some(Golden { samples, log }))
}

/// Compares a Rust run with the reference output. Non-finite samples on
/// either side are always a mismatch.
pub fn compare(rendered: &Rendered, golden: &Golden) -> Result<Stats, String> {
    if rendered.log != golden.log {
        return Err(format!(
            "log differs\n  rust: {:?}\n  c++:  {:?}",
            rendered.log, golden.log
        ));
    }
    if rendered.samples.len() != golden.samples.len() {
        return Err(format!(
            "rust rendered {} samples, c++ rendered {}",
            rendered.samples.len(),
            golden.samples.len()
        ));
    }
    let mut max_abs = 0.0_f32;
    let mut sum_squares = 0.0_f64;
    for (index, (&rust, &cpp)) in rendered.samples.iter().zip(&golden.samples).enumerate() {
        let difference = (rust - cpp).abs();
        if !rust.is_finite() || !cpp.is_finite() || difference > rendered.tolerance {
            return Err(format!(
                "sample {index}: rust {rust}, c++ {cpp} (tolerance {})",
                rendered.tolerance
            ));
        }
        max_abs = max_abs.max(difference);
        sum_squares += f64::from(difference) * f64::from(difference);
    }
    let samples = rendered.samples.len();
    let rms = if samples == 0 {
        0.0
    } else {
        (sum_squares / samples as f64).sqrt()
    };
    Ok(Stats {
        samples,
        max_abs,
        rms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rendered(samples: Vec<f32>) -> Rendered {
        Rendered {
            samples,
            log: vec!["init 0".to_owned()],
            tolerance: 1e-6,
        }
    }

    fn golden(samples: Vec<f32>) -> Golden {
        Golden {
            samples,
            log: vec!["init 0".to_owned()],
        }
    }

    #[test]
    fn matching_output_passes_with_stats() {
        let stats = compare(&rendered(vec![0.5, 0.25]), &golden(vec![0.5, 0.2500005])).unwrap();
        assert_eq!(stats.samples, 2);
        assert!(stats.max_abs > 0.0 && stats.max_abs <= 1e-6);
    }

    #[test]
    fn differences_beyond_tolerance_fail() {
        assert!(compare(&rendered(vec![0.5]), &golden(vec![0.6])).is_err());
        assert!(compare(&rendered(vec![0.5]), &golden(vec![0.5, 0.0])).is_err());
        assert!(compare(&rendered(vec![f32::NAN]), &golden(vec![f32::NAN])).is_err());
    }

    #[test]
    fn log_differences_fail() {
        let mut other = golden(vec![]);
        other.log = vec!["init 1".to_owned()];
        assert!(compare(&rendered(vec![]), &other).is_err());
    }
}
