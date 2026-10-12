//! Parser for `.scn` scenario files.

use std::fmt;
use std::str::FromStr;

/// A parsed scenario: a named list of steps.
#[derive(Clone, Debug, PartialEq)]
pub struct Scenario {
    /// File stem, used to find the matching C++ output.
    pub name: String,
    /// Steps in file order.
    pub steps: Vec<Step>,
}

/// One line of a scenario: an operation and its `key=value` arguments.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    /// 1-based line number in the scenario file.
    pub line: usize,
    /// Operation name.
    pub op: String,
    /// Arguments in file order; keys are unique.
    pub args: Vec<(String, String)>,
}

/// A scenario that could not be parsed or executed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenarioError {
    /// 1-based line number, or 0 when the error is not tied to a line.
    pub line: usize,
    /// What went wrong.
    pub message: String,
}

impl ScenarioError {
    /// Creates an error for `line`.
    pub fn new(line: usize, message: impl Into<String>) -> Self {
        Self {
            line,
            message: message.into(),
        }
    }
}

impl fmt::Display for ScenarioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ScenarioError {}

impl Step {
    /// Fails if any argument key is not in `allowed`.
    pub fn check_keys(&self, allowed: &[&str]) -> Result<(), ScenarioError> {
        match self
            .args
            .iter()
            .find(|(key, _)| !allowed.contains(&key.as_str()))
        {
            Some((key, _)) => {
                Err(self.error(format!("unknown argument `{key}` for `{}`", self.op)))
            }
            None => Ok(()),
        }
    }

    /// The raw value of `key`, if present.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.args
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, value)| value.as_str())
    }

    /// Parses `key` as `T`, or returns `default` when it is absent.
    pub fn value_or<T: FromStr>(&self, key: &str, default: T) -> Result<T, ScenarioError> {
        match self.get(key) {
            Some(raw) => raw
                .parse()
                .map_err(|_| self.error(format!("invalid value `{raw}` for `{key}`"))),
            None => Ok(default),
        }
    }

    /// Parses the required argument `key` as `T`.
    pub fn value<T: FromStr>(&self, key: &str) -> Result<T, ScenarioError> {
        let raw = self
            .get(key)
            .ok_or_else(|| self.error(format!("`{}` needs `{key}=`", self.op)))?;
        raw.parse()
            .map_err(|_| self.error(format!("invalid value `{raw}` for `{key}`")))
    }

    /// An error located at this step.
    pub fn error(&self, message: impl Into<String>) -> ScenarioError {
        ScenarioError::new(self.line, message)
    }
}

/// Parses scenario `text` named `name`.
pub fn parse(name: &str, text: &str) -> Result<Scenario, ScenarioError> {
    let mut steps = Vec::new();
    for (index, raw_line) in text.lines().enumerate() {
        let line = index + 1;
        let content = raw_line.trim();
        if content.is_empty() || content.starts_with('#') {
            continue;
        }
        let mut tokens = content.split_whitespace();
        let op = tokens.next().unwrap_or_default().to_owned();
        let mut args: Vec<(String, String)> = Vec::new();
        for token in tokens {
            let (key, value) = token
                .split_once('=')
                .filter(|(key, _)| !key.is_empty())
                .ok_or_else(|| {
                    ScenarioError::new(line, format!("expected key=value, got `{token}`"))
                })?;
            if args.iter().any(|(existing, _)| existing == key) {
                return Err(ScenarioError::new(
                    line,
                    format!("duplicate argument `{key}`"),
                ));
            }
            args.push((key.to_owned(), value.to_owned()));
        }
        steps.push(Step { line, op, args });
    }
    Ok(Scenario {
        name: name.to_owned(),
        steps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_steps_and_skips_comments() {
        let scenario = parse(
            "demo",
            "# comment\n\ninit channels=2 rate=48000\r\n  mix frames=16\n",
        )
        .unwrap();
        assert_eq!(scenario.name, "demo");
        assert_eq!(scenario.steps.len(), 2);
        assert_eq!(scenario.steps[0].line, 3);
        assert_eq!(scenario.steps[0].op, "init");
        assert_eq!(scenario.steps[0].get("rate"), Some("48000"));
        assert_eq!(scenario.steps[1].value::<u32>("frames"), Ok(16));
    }

    #[test]
    fn rejects_malformed_arguments() {
        assert_eq!(parse("x", "mix 16").unwrap_err().line, 1);
        assert!(parse("x", "mix =16").is_err());
        assert!(parse("x", "mix frames=1 frames=2").is_err());
    }

    #[test]
    fn reports_unknown_keys_and_bad_values() {
        let scenario = parse("x", "mix frames=abc speed=2").unwrap();
        let step = &scenario.steps[0];
        assert!(step.check_keys(&["frames"]).is_err());
        assert!(step.value::<u32>("frames").is_err());
        assert_eq!(step.value_or::<u32>("missing", 7), Ok(7));
    }
}
