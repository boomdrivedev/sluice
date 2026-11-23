//! `sluice equip` (147): the worktree it runs in, equipped for a branch's
//! runner as sluice equips every checkout before it runs anything there,
//! through the setup. It holds nothing: no ports, no record of the
//! worktree, no part of the host's bound (71; rules core.16, core.18),
//! and nothing of its own once it answers. The steps are the check
//! profile's, named as it runs them (rule core.1).

use std::io::Write;
use std::path::Path;

use anyhow::Result;
use sluice_core::equip::{EquipRefusal, Equipment};
use sluice_core::Sluice;
use sluice_seams::Rev;

use crate::exec::Host;
use crate::told::{self, Kind};
use crate::{Pushers, CONFLICT, FAILED, LANDED};

pub struct Equip<'a> {
    pub branch: String,
    /// The revision whose policy equips it; the branch's current one where
    /// none is named.
    pub at: Option<String>,
    pub json: bool,
    pub host: &'a dyn Host,
    /// Where each branch lives, to say so when it cannot be read.
    pub pushers: &'a dyn Pushers,
}

impl Equip<'_> {
    pub fn run(&self, s: &Sluice<'_>, out: &mut dyn Write) -> Result<i32> {
        let json = self.json;
        let refuse = |out: &mut dyn Write, text: &str, next: &[String]| -> Result<i32> {
            told::tell(out, json, Kind::Refused, text, next)?;
            Ok(CONFLICT)
        };
        let Some(working_copy) = self.host.working_copy(".")? else {
            let folder = crate::caller::folder()
                .map(|d| d.display().to_string())
                .unwrap_or_else(|_| ".".into());
            return refuse(out, &format!("refused: {folder} is in no worktree"), &[]);
        };
        let at = match &self.at {
            None => None,
            Some(name) => match self.host.revision(name)? {
                Some(full) => Some(Rev::new(full)),
                None => {
                    return refuse(out, &format!("refused: {name} names no revision here"), &[]);
                }
            },
        };
        let overview = ["sluice overview".to_string()];
        let equipment = match s.equip(&self.branch, at.as_ref(), Path::new(&working_copy))? {
            Ok(e) => e,
            Err(EquipRefusal::NoBranch) => {
                return refuse(
                    out,
                    &crate::no_branch(&self.branch, self.pushers)?,
                    &overview,
                );
            }
            Err(EquipRefusal::NoPolicy(rev)) => {
                let text = format!("refused: no .config/sluice.toml at {}", rev.short());
                return refuse(out, &text, &[]);
            }
            Err(EquipRefusal::NotManaged(rev)) => {
                let text = format!(
                    "refused: no [branches] table in .config/sluice.toml at {} names {}",
                    rev.short(),
                    self.branch
                );
                return refuse(out, &text, &overview);
            }
        };
        match json {
            true => writeln!(out, "{}", document(&equipment))?,
            false => write_text(&equipment, &working_copy, out)?,
        }
        Ok(match equipment.equipped.failed() {
            Some(_) => FAILED,
            None => LANDED,
        })
    }
}

/// `--json`'s document: the branch, the revision whose policy was read,
/// and each step as it ran.
fn document(e: &Equipment) -> serde_json::Value {
    serde_json::json!({
        "branch": e.branch,
        "revision": e.revision,
        "steps": e.equipped.steps,
    })
}

/// For a person: what is equipped, then each step and its outcome, a
/// failed step's words beneath it.
fn write_text(e: &Equipment, working_copy: &str, out: &mut dyn Write) -> Result<()> {
    writeln!(
        out,
        "equipping {working_copy} for {} at {}",
        e.branch,
        e.revision.short()
    )?;
    if e.equipped.steps.is_empty() {
        writeln!(
            out,
            "nothing to equip: {}'s policy names no toolchain, and its runner nothing to set up",
            e.branch
        )?;
    }
    for step in &e.equipped.steps {
        let outcome = if step.passed { "passed" } else { "failed" };
        writeln!(out, "{}: {outcome}", step.step)?;
        if !step.passed {
            for line in step.words.lines() {
                writeln!(out, "  {line}")?;
            }
        }
    }
    Ok(())
}
