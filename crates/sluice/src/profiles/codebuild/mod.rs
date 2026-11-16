//! The check seam's launched profile on an AWS CodeBuild reserved-capacity
//! fleet (88, 107, 111, C.3's launched row; record 0119; the change
//! `a-codebuild-fleet-runs-a-queues-checks`): a run is one build of its
//! system's run project, which holds no key, started by the lead once the
//! run is claimed, with the claim and nothing else; its end starts a build
//! of the sign project through the rule the stack writes, which runs none
//! of the combination's code. Where a claim's job stands is read from the
//! two builds (design D2, D3, D5).
//!
//! The host keeps each claim's builds by their ids in its own state,
//! `codebuild/<h>.json` under it, `<h>` the first 32 hex digits of the
//! SHA-256 of the repository and the claim, as it keeps the forge's
//! answers: they decide nothing, and a lead that keeps none finds the
//! builds by listing the projects' (rule core.32's way).

pub mod aws;
#[cfg(test)]
pub mod fake;
pub mod sdk;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use sluice_seams::{Job, Launch, Setting};

use self::aws::{Aws, Build};
use crate::profiles::checks::Launcher;

/// The variables a run build is started with, and no other (design D3).
pub const CLAIM: &str = "SLUICE_CLAIM";
pub const BRANCH: &str = "SLUICE_BRANCH";
pub const COMBINATION: &str = "SLUICE_COMBINATION";
pub const SYSTEM: &str = "SLUICE_SYSTEM";
pub const READ_TOKEN: &str = "SLUICE_READ_TOKEN";

/// The variable the rule starts a sign build with: the run build's ARN
/// (design D2).
pub const RUN_BUILD: &str = "SLUICE_RUN_BUILD";

/// How the profile reaches AWS in a region.
pub type Reach = Box<dyn Fn(&str) -> Rc<dyn Aws>>;

/// How the profile buys the token a run build reads its repository with,
/// for the remote a branch is on (design D4).
pub type Reader = Box<dyn Fn(Option<&str>) -> Result<String>>;

/// A `codebuild = { region, sign }` table: where the projects are, and
/// the sign project.
pub fn table(kind: &(String, Setting)) -> Result<(String, String)> {
    if kind.0 != "codebuild" {
        bail!("a {} pool is not a CodeBuild fleet", kind.0);
    }
    let Setting::Tables(t) = &kind.1 else {
        bail!("codebuild must be a table naming its region and sign project");
    };
    let text = |key: &str| match t.first().and_then(|t| t.get(key)) {
        Some(Setting::Text(v)) => Ok(v.clone()),
        _ => Err(anyhow!("the codebuild table names no {key}")),
    };
    Ok((text("region")?, text("sign")?))
}

/// The builds the host keeps for a claim.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Kept {
    claim: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    run: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sign: Option<String>,
}

pub struct CodeBuild {
    reach: Reach,
    /// `None` on a host whose `[forge]` names no app.
    reader: Option<Reader>,
    /// The repository's common git directory, which tells its claims from
    /// another repository's in the host's state.
    common: PathBuf,
    /// Where the host keeps each claim's builds.
    kept: PathBuf,
    /// `[peers] alive`: how long a run build may wait for its sign build.
    alive: Duration,
    now: Box<dyn Fn() -> DateTime<Utc>>,
}

impl CodeBuild {
    /// The pool of the repository whose common directory is `common`,
    /// reaching AWS by `reach`, buying read tokens by `reader`, keeping its
    /// builds under `state`, waiting `alive` for a sign build.
    pub fn new(
        reach: Reach,
        reader: Option<Reader>,
        common: &Path,
        state: &Path,
        alive: Duration,
    ) -> CodeBuild {
        CodeBuild {
            reach,
            reader,
            common: common.to_path_buf(),
            kept: state.join("codebuild"),
            alive,
            now: Box::new(Utc::now),
        }
    }

    /// The same pool, reading the time from `now`: for tests.
    pub fn at(mut self, now: impl Fn() -> DateTime<Utc> + 'static) -> CodeBuild {
        self.now = Box::new(now);
        self
    }

    fn kept_path(&self, claim: &str) -> PathBuf {
        let at = format!("{}\n{claim}", self.common.display());
        let h = crate::profiles::remote_refs::sha256_hex(at.as_bytes());
        self.kept.join(format!("{}.json", &h[..32]))
    }

    /// What the host keeps for `claim`; nothing where it keeps nothing or
    /// what it keeps does not parse.
    fn kept(&self, claim: &str) -> Kept {
        std::fs::read(self.kept_path(claim))
            .ok()
            .and_then(|b| serde_json::from_slice::<Kept>(&b).ok())
            .filter(|k| k.claim == claim)
            .unwrap_or_else(|| Kept {
                claim: claim.into(),
                ..Kept::default()
            })
    }

    /// Keep `kept`, written whole and renamed over the last. It decides
    /// nothing, so a write that fails is passed over.
    fn keep(&self, kept: &Kept) {
        let path = self.kept_path(&kept.claim);
        let write = || -> Result<()> {
            std::fs::create_dir_all(&self.kept)?;
            let tmp = path.with_extension(format!("json.{}", std::process::id()));
            std::fs::write(&tmp, serde_json::to_vec(kept)?)?;
            std::fs::rename(&tmp, &path)?;
            Ok(())
        };
        if let Err(e) = write() {
            tracing::debug!("could not keep {}'s builds: {e:#}", kept.claim);
        }
    }

    /// The run build that bears `claim`: the one the host keeps, or the
    /// newest of the run projects' that bears it.
    fn run_build(
        &self,
        aws: &dyn Aws,
        runners: &BTreeMap<String, String>,
        kept: &mut Kept,
    ) -> Result<Option<Build>> {
        if let Some(id) = &kept.run {
            if let Some(b) = aws.batch_get_builds(std::slice::from_ref(id))?.pop() {
                return Ok(Some(b));
            }
        }
        let mut projects: Vec<&String> = runners.values().collect();
        projects.sort();
        projects.dedup();
        let mut ids = Vec::new();
        for project in projects {
            ids.extend(aws.list_builds_for_project(project)?.into_iter().take(100));
        }
        let found = newest(aws, &ids, |b| b.variables.get(CLAIM) == Some(&kept.claim))?;
        kept.run = found.as_ref().map(|b| b.id.clone());
        Ok(found)
    }

    /// The sign build that bears `run`: the one the host keeps, or the
    /// newest of the sign project's that bears it.
    fn sign_build(
        &self,
        aws: &dyn Aws,
        sign: &str,
        run: &Build,
        kept: &mut Kept,
    ) -> Result<Option<Build>> {
        if let Some(id) = &kept.sign {
            if let Some(b) = aws.batch_get_builds(std::slice::from_ref(id))?.pop() {
                return Ok(Some(b));
            }
        }
        let ids: Vec<String> = aws
            .list_builds_for_project(sign)?
            .into_iter()
            .take(100)
            .collect();
        let found = newest(aws, &ids, |b| {
            b.variables
                .get(RUN_BUILD)
                .is_some_and(|r| *r == run.arn || *r == run.id)
        })?;
        kept.sign = found.as_ref().map(|b| b.id.clone());
        Ok(found)
    }
}

/// The newest of `ids`, read in one `BatchGetBuilds`, for which `bears`
/// holds; `ids` newest first, as CodeBuild lists them.
fn newest(aws: &dyn Aws, ids: &[String], bears: impl Fn(&Build) -> bool) -> Result<Option<Build>> {
    if ids.is_empty() {
        return Ok(None);
    }
    let mut builds: Vec<Build> = aws
        .batch_get_builds(ids)?
        .into_iter()
        .filter(|b| bears(b))
        .collect();
    builds.sort_by_key(|b| std::cmp::Reverse(b.start));
    Ok(builds.into_iter().next())
}

/// A time as the records write one.
fn stamp(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Secs, true)
}

impl Launcher for CodeBuild {
    /// 88, record 0122 (design D3, D4): one `StartBuild` of the run project
    /// `runners` names for the run's system, with the claim, the branch,
    /// the combination, the system and a token that reads the branch's
    /// repository for an hour, the claim its idempotency token, and nothing
    /// else; the build CodeBuild answers kept in the host's state. A host
    /// with no app starts nothing.
    fn launch(&self, launch: &Launch) -> Result<()> {
        let (region, _) = table(&launch.kind)?;
        let Some(reader) = &self.reader else {
            bail!(
                "[forge] names no app on this host: a run build on the pool {} reads {}'s \
                 repository with a token of sluice's app that its lead buys, so a host that \
                 leads a branch on a CodeBuild pool names the app in [forge]",
                launch.pool,
                launch.branch
            );
        };
        let token = reader(launch.remote.as_deref())
            .with_context(|| format!("buying the token run build {} reads with", launch.claim))?;
        let variables = [
            (CLAIM, launch.claim.clone()),
            (BRANCH, launch.branch.clone()),
            (COMBINATION, launch.combination.as_str().to_string()),
            (SYSTEM, launch.system.clone().unwrap_or_default()),
            (READ_TOKEN, token),
        ]
        .map(|(n, v)| (n.to_string(), v));
        let aws = (self.reach)(&region);
        let build = aws.start_build(&launch.runner, &variables, &launch.claim)?;
        self.keep(&Kept {
            claim: launch.claim.clone(),
            run: Some(build.id),
            sign: None,
        });
        Ok(())
    }

    /// 77, 88, record 0122 (design D5): where `claim`'s builds stand. None
    /// where no run build bears it; queued while it is submitted or
    /// queued; running from its queued phase's end until a sign build of
    /// it completes; ended, recorded where that sign build succeeded. A run
    /// build stopped, or ended with no sign build bearing it within `[peers]
    /// alive`, ended with no record, saying why.
    fn job(
        &self,
        kind: &(String, Setting),
        runners: &BTreeMap<String, String>,
        claim: &str,
        _remote: Option<&str>,
    ) -> Result<Job> {
        let (region, sign) = table(kind)?;
        let aws = (self.reach)(&region);
        let mut kept = self.kept(claim);
        let before = kept.clone();
        let job = (|| -> Result<Job> {
            let Some(run) = self.run_build(aws.as_ref(), runners, &mut kept)? else {
                return Ok(Job::None);
            };
            if !run.complete && matches!(run.phase.as_str(), "SUBMITTED" | "QUEUED" | "") {
                return Ok(Job::Queued);
            }
            let since = stamp(run.queued_end.or(run.start).unwrap_or_else(|| (self.now)()));
            if !run.complete {
                return Ok(Job::Running { since });
            }
            if run.status == "STOPPED" {
                return Ok(Job::Ended {
                    recorded: false,
                    why: Some(format!(
                        "{claim}'s run build {} was stopped, which starts no sign build",
                        run.id
                    )),
                });
            }
            match self.sign_build(aws.as_ref(), &sign, &run, &mut kept)? {
                Some(s) if s.complete => Ok(Job::Ended {
                    recorded: s.status == "SUCCEEDED",
                    why: (s.status != "SUCCEEDED").then(|| {
                        format!(
                            "{claim}'s sign build {} ended {} with no record of its run",
                            s.id, s.status
                        )
                    }),
                }),
                Some(_) => Ok(Job::Running { since }),
                None => {
                    let ended = run.end.unwrap_or_else(|| (self.now)());
                    let waited = ((self.now)() - ended).to_std().unwrap_or_default();
                    if waited < self.alive {
                        return Ok(Job::Running { since });
                    }
                    Ok(Job::Ended {
                        recorded: false,
                        why: Some(format!(
                            "{claim}'s run build {} ended {} and no build of {sign} took it \
                             within [peers] alive ({}): the EventBridge rule that starts {sign} \
                             when a run build ends is missing or broken",
                            run.id,
                            run.status,
                            crate::progress::duration(self.alive.as_secs() as i64)
                        )),
                    })
                }
            }
        })();
        if kept != before {
            self.keep(&kept);
        }
        job
    }

    /// 111 (design D5): nothing is started; no build runs a round, so a
    /// wake is lost, which only delays a submission (59).
    fn wake(&self, _kind: &(String, Setting), _branch: &str, _remote: Option<&str>) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::fake::Fake;
    use super::*;

    const RUN: &str = "sluice-run-linux-x64";

    fn kind() -> (String, Setting) {
        let t = BTreeMap::from([
            ("region".to_string(), Setting::Text("us-east-1".into())),
            ("sign".to_string(), Setting::Text("sluice-sign".into())),
        ]);
        ("codebuild".into(), Setting::Tables(vec![t]))
    }

    fn runners() -> BTreeMap<String, String> {
        BTreeMap::from([("linux-x64".to_string(), RUN.to_string())])
    }

    /// A scratch directory of the test's own, removed when it ends.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let dir = std::env::temp_dir()
                .join(format!("sluice-codebuild-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// The pool over `aws`, its state in `scratch`, buying `ghs_reads`.
    fn pool(aws: &Rc<Fake>, scratch: &Scratch) -> CodeBuild {
        let aws = aws.clone();
        CodeBuild::new(
            Box::new(move |_| aws.clone() as Rc<dyn Aws>),
            Some(Box::new(|_| Ok("ghs_reads".into()))),
            &scratch.0.join("repo.git"),
            &scratch.0.join("state"),
            Duration::from_secs(120),
        )
    }

    fn launch(claim: &str) -> Launch {
        Launch {
            pool: "codebuild".into(),
            kind: kind(),
            signer: ("kms".into(), Setting::Text(String::new())),
            runner: RUN.into(),
            remote: Some("origin".into()),
            branch: "main".into(),
            candidate: "c3".into(),
            combination: sluice_seams::Rev::new("c0ffee"),
            system: Some("linux-x64".into()),
            claim: claim.into(),
        }
    }

    /// Record 0119, 88 (design D3, D4): one `StartBuild` of the run
    /// project for the run's system, carrying the five variables and the
    /// claim as its token and no other override; a host with no app starts
    /// nothing, naming `[forge]`.
    #[test]
    fn a_run_build_is_started_with_its_claim_and_nothing_else() {
        let scratch = Scratch::new("start");
        let aws = Rc::new(Fake::new());
        pool(&aws, &scratch)
            .launch(&launch("pool/codebuild/4"))
            .unwrap();
        assert_eq!(aws.calls(), [format!("StartBuild {RUN}")]);
        let s = aws.state();
        assert_eq!(s.builds.len(), 1);
        let b = &s.builds[0];
        assert_eq!(b.project, RUN);
        assert_eq!(
            b.variables,
            BTreeMap::from([
                (CLAIM.to_string(), "pool/codebuild/4".to_string()),
                (BRANCH.to_string(), "main".to_string()),
                (COMBINATION.to_string(), "c0ffee".to_string()),
                (SYSTEM.to_string(), "linux-x64".to_string()),
                (READ_TOKEN.to_string(), "ghs_reads".to_string()),
            ])
        );
        assert_eq!(s.tokens["pool/codebuild/4"], b.id);
        drop(s);

        let aws = Rc::new(Fake::new());
        let reach = aws.clone();
        let no_app = CodeBuild::new(
            Box::new(move |_| reach.clone() as Rc<dyn Aws>),
            None,
            &scratch.0.join("repo.git"),
            &scratch.0.join("state"),
            Duration::from_secs(120),
        );
        let e = format!(
            "{:#}",
            no_app.launch(&launch("pool/codebuild/5")).unwrap_err()
        );
        assert!(e.contains("[forge]"), "{e}");
        assert!(aws.calls().is_empty());
    }

    /// Record 0122 (design D3; rule core.29): a start whose answer was lost
    /// keeps nothing, and the next round finds the build that bears the
    /// claim by listing the run project's, so the lead starts nothing more;
    /// a start sent again under the claim starts no second build.
    #[test]
    fn a_run_build_found_is_not_started_again() {
        let scratch = Scratch::new("found");
        let aws = Rc::new(Fake::new());
        aws.put_build(RUN, &[(CLAIM, "pool/codebuild/3"), (BRANCH, "main")]);
        let started = aws.put_build(RUN, &[(CLAIM, "pool/codebuild/4"), (BRANCH, "main")]);
        aws.put_build(RUN, &[(CLAIM, "pool/codebuild/5"), (BRANCH, "main")]);
        let p = pool(&aws, &scratch);
        assert_eq!(
            p.job(&kind(), &runners(), "pool/codebuild/4", None)
                .unwrap(),
            Job::Queued
        );
        assert!(aws.calls().contains(&format!("ListBuildsForProject {RUN}")));
        assert!(!aws.calls().iter().any(|c| c.starts_with("StartBuild")));
        // Kept once found: read by its id alone after.
        let before = aws.calls().len();
        p.job(&kind(), &runners(), "pool/codebuild/4", None)
            .unwrap();
        assert_eq!(
            aws.calls()[before..],
            [format!("BatchGetBuilds {}", started.id)]
        );
        assert_eq!(
            p.job(&kind(), &runners(), "pool/codebuild/9", None)
                .unwrap(),
            Job::None
        );

        // A start sent again within CodeBuild's window starts nothing new.
        p.launch(&launch("pool/codebuild/7")).unwrap();
        p.launch(&launch("pool/codebuild/7")).unwrap();
        assert_eq!(aws.state().builds.len(), 4);
    }
}
