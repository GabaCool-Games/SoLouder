//! Per-platform tolerances: `tests/scenarios/tolerances.txt` declares, for a
//! named comparison profile, the scenarios that may differ from the
//! reference output and by how much. Every scenario not listed must still
//! match exactly (or within its own `tolerance`).
//!
//! Each line is `<profile> <scenario> <max abs diff>`, optionally followed by
//! `# why`; blank lines and lines starting with `#` are ignored. A profile
//! with no lines declares nothing: every scenario must match exactly.

use std::collections::HashMap;

/// The tolerances `text` declares for `profile`, by scenario name.
pub fn tolerances(text: &str, profile: &str) -> Result<HashMap<String, f32>, String> {
    let mut found = HashMap::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or_default().trim();
        if line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        let [name, scenario, value] = fields[..] else {
            return Err(format!(
                "line {}: expected `profile scenario max`",
                index + 1
            ));
        };
        let value: f32 = value
            .parse()
            .ok()
            .filter(|v: &f32| v.is_finite() && *v > 0.0)
            .ok_or_else(|| format!("line {}: `{value}` is not a positive number", index + 1))?;
        if name == profile && found.insert(scenario.to_owned(), value).is_some() {
            return Err(format!("line {}: `{scenario}` listed twice", index + 1));
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "# comment\ngcc a 1e-6 # why\n\ngcc b 2e-4\ncross a 1e-7\n";

    #[test]
    fn a_profile_lists_its_scenarios() {
        let gcc = tolerances(TEXT, "gcc").unwrap();
        assert_eq!((gcc["a"], gcc["b"], gcc.len()), (1e-6, 2e-4, 2));
        assert_eq!(tolerances(TEXT, "cross").unwrap()["a"], 1e-7);
    }

    #[test]
    fn other_profiles_declare_nothing_and_bad_lines_are_errors() {
        assert!(tolerances(TEXT, "macos").unwrap().is_empty());
        assert!(tolerances("gcc a\n", "gcc").is_err());
        assert!(tolerances("gcc a 0\n", "gcc").is_err());
        assert!(tolerances("gcc a 1e-6\ngcc a 1e-6\n", "gcc").is_err());
    }
}
