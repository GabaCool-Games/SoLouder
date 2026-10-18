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
    /// Samples that match only because one side is subnormal and the other
    /// zero.
    pub flushed: usize,
    /// Samples whose bits differ (after counting subnormals as zero); within
    /// a tolerance they may still match.
    pub differing: usize,
}

/// Zero for any zero or subnormal value: the C++ reference runs with
/// denormals flushed to zero, so it can write zero where Rust keeps a
/// subnormal.
fn flush(sample: f32) -> f32 {
    if sample == 0.0 || sample.is_subnormal() {
        0.0
    } else {
        sample
    }
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

/// Compares a Rust run with the reference output at the scenario's own
/// tolerance; see [`compare_within`].
pub fn compare(rendered: &Rendered, golden: &Golden) -> Result<Stats, String> {
    compare_within(rendered, golden, rendered.tolerance)
}

/// Compares a Rust run with the reference output. With a tolerance of 0 the
/// samples must have identical bits, except that zero and subnormal values
/// all count as zero; otherwise they may differ by up to the tolerance.
/// Non-finite samples on either side, and scenarios without samples, are
/// always a mismatch. A mismatch reports the first offending sample and the
/// statistics over every sample.
pub fn compare_within(
    rendered: &Rendered,
    golden: &Golden,
    tolerance: f32,
) -> Result<Stats, String> {
    if rendered.log != golden.log {
        return Err(format!(
            "log differs
  rust: {:?}
  c++:  {:?}",
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
    if rendered.samples.is_empty() {
        return Err("the scenario renders no samples, so it checks no audio".to_owned());
    }
    let exact = tolerance == 0.0;
    let mut max_abs = 0.0_f32;
    let mut sum_squares = 0.0_f64;
    let (mut flushed, mut differing, mut failing) = (0, 0, 0);
    let mut first = None;
    for (index, (&rust, &cpp)) in rendered.samples.iter().zip(&golden.samples).enumerate() {
        let difference = (rust - cpp).abs();
        let same = flush(rust).to_bits() == flush(cpp).to_bits();
        let matches = same || (!exact && difference <= tolerance);
        if !rust.is_finite() || !cpp.is_finite() || !matches {
            failing += 1;
            first.get_or_insert(format!(
                "sample {index}: rust {rust:e} ({:08x}), c++ {cpp:e} ({:08x})",
                rust.to_bits(),
                cpp.to_bits(),
            ));
        }
        if same && rust.to_bits() != cpp.to_bits() {
            flushed += 1;
        }
        differing += usize::from(!same);
        if difference.is_finite() {
            max_abs = max_abs.max(difference);
            sum_squares += f64::from(difference) * f64::from(difference);
        }
    }
    let samples = rendered.samples.len();
    let stats = Stats {
        samples,
        max_abs,
        rms: (sum_squares / samples as f64).sqrt(),
        flushed,
        differing,
    };
    match first {
        None => Ok(stats),
        Some(first) => Err(format!(
            "{failing} of {samples} samples outside tolerance {tolerance:e} ({differing} differ; max abs diff {:e}, rms {:e}); first at {first}",
            stats.max_abs, stats.rms
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rendered(samples: Vec<f32>) -> Rendered {
        Rendered {
            samples,
            log: vec!["init 0".to_owned()],
            tolerance: 0.0,
        }
    }

    fn tolerant(samples: Vec<f32>) -> Rendered {
        Rendered {
            tolerance: 1e-6,
            ..rendered(samples)
        }
    }

    fn golden(samples: Vec<f32>) -> Golden {
        Golden {
            samples,
            log: vec!["init 0".to_owned()],
        }
    }

    #[test]
    fn identical_output_passes_exactly() {
        let stats = compare(&rendered(vec![0.5, -0.25]), &golden(vec![0.5, -0.25])).unwrap();
        assert_eq!((stats.samples, stats.max_abs, stats.flushed), (2, 0.0, 0));
    }

    #[test]
    fn any_difference_fails_without_a_tolerance() {
        let next = f32::from_bits(0.25_f32.to_bits() + 1);
        assert!(compare(&rendered(vec![0.25]), &golden(vec![next])).is_err());
        assert!(compare(&rendered(vec![f32::MIN_POSITIVE]), &golden(vec![0.0])).is_err());
    }

    #[test]
    fn subnormals_match_zero() {
        let tiny = f32::from_bits(1);
        let stats = compare(
            &rendered(vec![tiny, -tiny, 0.0, -0.0]),
            &golden(vec![0.0, 0.0, -0.0, tiny]),
        )
        .unwrap();
        assert_eq!(stats.flushed, 4);
    }

    #[test]
    fn a_tolerance_allows_small_differences() {
        let stats = compare(&tolerant(vec![0.5, 0.25]), &golden(vec![0.5, 0.2500005])).unwrap();
        assert!(stats.max_abs > 0.0 && stats.max_abs <= 1e-6);
        assert!(compare(&tolerant(vec![0.5]), &golden(vec![0.6])).is_err());
    }

    #[test]
    fn length_and_non_finite_differences_fail() {
        assert!(compare(&rendered(vec![0.5]), &golden(vec![0.5, 0.0])).is_err());
        assert!(compare(&rendered(vec![f32::NAN]), &golden(vec![f32::NAN])).is_err());
        let infinity = f32::INFINITY;
        assert!(compare(&rendered(vec![infinity]), &golden(vec![infinity])).is_err());
    }

    #[test]
    fn scenarios_without_samples_fail() {
        assert!(compare(&rendered(vec![]), &golden(vec![])).is_err());
    }

    #[test]
    fn log_differences_fail() {
        let mut other = golden(vec![0.0]);
        other.log = vec!["init 1".to_owned()];
        assert!(compare(&rendered(vec![0.0]), &other).is_err());
    }
}
