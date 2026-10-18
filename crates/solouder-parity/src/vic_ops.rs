//! Scenario operations on `Vic`: `vic`, `vic_set` and `vic_query`. Each
//! mirrors the operation of the same name in
//! `reference/tests/parity_generators.inc`.

use std::sync::Arc;

use solouder_sources::{VIC_REGISTERS, Vic, VicModel};

use crate::generator_ops::check_generator_keys;
use crate::run::State;
use crate::scenario::{ScenarioError, Step};
use crate::spatial_ops::group;

const VIC_KEYS: [&str; 8] = [
    "name", "model", "bass", "alto", "soprano", "noise", "reg", "value",
];

const REGISTERS: [&str; VIC_REGISTERS] = ["bass", "alto", "soprano", "noise"];

/// A `key` of 0 to 255.
fn byte(step: &Step, key: &str) -> Result<u8, ScenarioError> {
    let value: u32 = step.value(key)?;
    u8::try_from(value).map_err(|_| step.error(format!("`{key}` must be 0 to 255")))
}

/// `model` (0 PAL or 1 NTSC), then `bass`, `alto`, `soprano`, `noise` and
/// `reg` with `value`, each as given. A `reg` past the last is ignored, as
/// in the patched reference.
fn apply_vic(step: &Step, vic: &Vic) -> Result<(), ScenarioError> {
    if step.get("model").is_some() {
        let model = VicModel::from_index(step.value("model")?)
            .ok_or_else(|| step.error("`model` must be 0 or 1"))?;
        vic.set_model(model);
    }
    for (index, key) in REGISTERS.iter().enumerate() {
        if step.get(key).is_some() {
            vic.set_register(index, byte(step, key)?).expect("in range");
        }
    }
    if group(step, &["reg", "value"], "`reg` and `value`")? {
        let reg: u32 = step.value("reg")?;
        let _ = vic.set_register(reg as usize, byte(step, "value")?);
    }
    Ok(())
}

impl State {
    pub(crate) fn vic_op(&mut self, step: &Step) -> Result<(), ScenarioError> {
        match step.op.as_str() {
            "vic" => {
                check_generator_keys(step, &VIC_KEYS)?;
                self.mixer(step)?;
                let vic = Vic::new();
                apply_vic(step, &vic)?;
                self.store_generator(step, Arc::new(vic))
            }
            "vic_set" => {
                step.check_keys(&VIC_KEYS)?;
                let vic = self.generator::<Vic>(step, "vic")?;
                apply_vic(step, &vic)
            }
            "vic_query" => {
                step.check_keys(&["name", "reg"])?;
                let vic = self.generator::<Vic>(step, "vic")?;
                let regs: Vec<String> = (0..VIC_REGISTERS)
                    .map(|i| vic.register(i).unwrap_or(0).to_string())
                    .collect();
                let mut line = format!(
                    "vic_query {} model={} regs={}",
                    step.text("name")?,
                    vic.model().index(),
                    regs.join(",")
                );
                if step.get("reg").is_some() {
                    let reg: u32 = step.value("reg")?;
                    let value = vic.register(reg as usize).unwrap_or(0);
                    line += &format!(" reg={value}");
                }
                self.log(line);
                Ok(())
            }
            other => Err(step.error(format!("unsupported operation `{other}`"))),
        }
    }
}
