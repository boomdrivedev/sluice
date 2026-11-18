//! AWS as the profile's tests need it (design D9 of the change
//! `a-codebuild-fleet-runs-a-queues-checks`): builds started and read,
//! projects, fleets, objects and a P-256 key in place of CodeBuild's, S3's
//! and KMS's, so no test reaches AWS.

use std::cell::{RefCell, RefMut};
use std::collections::BTreeMap;

use anyhow::{anyhow, bail, Result};
use p256::ecdsa::signature::hazmat::PrehashSigner as _;
use p256::ecdsa::{Signature, SigningKey};

use super::aws::{Aws, Build, Fleet, Project, PublicKey};

/// The account the fake's resources are in.
pub const ACCOUNT: &str = "111122223333";

/// What the fake holds, and every call made of it.
#[derive(Default)]
pub struct State {
    /// Each build, in the order started.
    pub builds: Vec<Build>,
    pub projects: BTreeMap<String, Project>,
    pub fleets: BTreeMap<String, Fleet>,
    /// Each object by its bucket and key.
    pub objects: BTreeMap<(String, String), Vec<u8>>,
    /// The key, by its ARN and its alias.
    pub key: Option<(String, String, SigningKey)>,
    pub identity: String,
    /// Every call, by its operation and what it named.
    pub calls: Vec<String>,
    /// Where set, every read fails with these words, as AWS refusing it.
    pub refusing: Option<String>,
    /// Each start's idempotency token and the build it started.
    pub tokens: BTreeMap<String, String>,
}

#[derive(Default)]
pub struct Fake(RefCell<State>);

impl Fake {
    pub fn new() -> Fake {
        Fake(RefCell::new(State {
            identity: format!("arn:aws:sts::{ACCOUNT}:assumed-role/sluice-lead/host"),
            ..State::default()
        }))
    }

    /// The fake holding a P-256 key aliased `alias`.
    pub fn with_key(self, alias: &str) -> Fake {
        let arn =
            format!("arn:aws:kms:us-east-1:{ACCOUNT}:key/1234abcd-12ab-34cd-56ef-1234567890ab");
        self.0.borrow_mut().key =
            Some((arn, alias.into(), SigningKey::random(&mut rand_core::OsRng)));
        self
    }

    pub fn state(&self) -> RefMut<'_, State> {
        self.0.borrow_mut()
    }

    /// The build `id`, changed by `change`.
    pub fn change(&self, id: &str, change: impl FnOnce(&mut Build)) {
        let mut s = self.0.borrow_mut();
        let b = s
            .builds
            .iter_mut()
            .find(|b| b.id == id)
            .unwrap_or_else(|| panic!("no build {id}"));
        change(b);
    }

    /// A build of `project` the fake holds, as if another started it.
    pub fn put_build(&self, project: &str, variables: &[(&str, &str)]) -> Build {
        let mut s = self.0.borrow_mut();
        let b = new_build(
            &s,
            project,
            variables
                .iter()
                .map(|(n, v)| (n.to_string(), v.to_string())),
        );
        s.builds.push(b.clone());
        b
    }

    /// Each call made, in order.
    pub fn calls(&self) -> Vec<String> {
        self.0.borrow().calls.clone()
    }

    fn read(&self, call: String) -> Result<()> {
        let mut s = self.0.borrow_mut();
        s.calls.push(call.clone());
        match &s.refusing {
            Some(words) => bail!("{call}: {words}"),
            None => Ok(()),
        }
    }
}

fn new_build(s: &State, project: &str, variables: impl Iterator<Item = (String, String)>) -> Build {
    let id = format!("{project}:{:08}", s.builds.len() + 1);
    Build {
        arn: format!("arn:aws:codebuild:us-east-1:{ACCOUNT}:build/{id}"),
        id,
        project: project.into(),
        status: "IN_PROGRESS".into(),
        phase: "SUBMITTED".into(),
        complete: false,
        start: Some(chrono::Utc::now()),
        variables: variables.collect(),
        ..Build::default()
    }
}

impl Aws for Fake {
    fn start_build(
        &self,
        project: &str,
        variables: &[(String, String)],
        token: &str,
    ) -> Result<Build> {
        let mut s = self.0.borrow_mut();
        s.calls.push(format!("StartBuild {project}"));
        if let Some(id) = s.tokens.get(token).cloned() {
            return Ok(s
                .builds
                .iter()
                .find(|b| b.id == id)
                .cloned()
                .expect("a build"));
        }
        let b = new_build(&s, project, variables.iter().cloned());
        s.tokens.insert(token.into(), b.id.clone());
        s.builds.push(b.clone());
        Ok(b)
    }

    fn list_builds_for_project(&self, project: &str) -> Result<Vec<String>> {
        self.read(format!("ListBuildsForProject {project}"))?;
        let s = self.0.borrow();
        Ok(s.builds
            .iter()
            .rev()
            .filter(|b| b.project == project)
            .map(|b| b.id.clone())
            .collect())
    }

    fn batch_get_builds(&self, ids: &[String]) -> Result<Vec<Build>> {
        self.read(format!("BatchGetBuilds {}", ids.join(" ")))?;
        let s = self.0.borrow();
        Ok(ids
            .iter()
            .filter_map(|id| {
                s.builds
                    .iter()
                    .find(|b| &b.id == id || &b.arn == id)
                    .cloned()
            })
            .collect())
    }

    fn batch_get_projects(&self, names: &[String]) -> Result<Vec<Project>> {
        self.read(format!("BatchGetProjects {}", names.join(" ")))?;
        let s = self.0.borrow();
        Ok(names
            .iter()
            .filter_map(|n| s.projects.get(n).cloned())
            .collect())
    }

    fn batch_get_fleets(&self, arns: &[String]) -> Result<Vec<Fleet>> {
        self.read(format!("BatchGetFleets {}", arns.join(" ")))?;
        let s = self.0.borrow();
        Ok(arns
            .iter()
            .filter_map(|a| s.fleets.get(a).cloned())
            .collect())
    }

    fn get_object(&self, bucket: &str, key: &str) -> Result<Vec<u8>> {
        self.read(format!("GetObject s3://{bucket}/{key}"))?;
        let s = self.0.borrow();
        s.objects
            .get(&(bucket.to_string(), key.to_string()))
            .cloned()
            .ok_or_else(|| anyhow!("NoSuchKey: s3://{bucket}/{key}"))
    }

    fn sign(&self, key: &str, digest: &[u8; 32]) -> Result<Vec<u8>> {
        self.read(format!(
            "Sign {key} {}",
            crate::profiles::remote_refs::sha256_hex(digest)
        ))?;
        let s = self.0.borrow();
        let Some((arn, _, signing)) = &s.key else {
            bail!("NotFoundException: no key {key}");
        };
        if arn != key {
            bail!("NotFoundException: no key {key}");
        }
        let sig: Signature = signing.sign_prehash(digest)?;
        Ok(sig.to_der().as_bytes().to_vec())
    }

    fn get_public_key(&self, key: &str) -> Result<PublicKey> {
        self.read(format!("GetPublicKey {key}"))?;
        let s = self.0.borrow();
        let Some((arn, alias, signing)) = &s.key else {
            bail!("NotFoundException: no key {key}");
        };
        if key != arn && key != alias {
            bail!("NotFoundException: no key {key}");
        }
        Ok(PublicKey {
            arn: arn.clone(),
            spec: "ECC_NIST_P256".into(),
            usage: "SIGN_VERIFY".into(),
            der: crate::signing::kms::tests::spki(signing),
        })
    }

    fn caller_identity(&self) -> Result<String> {
        self.read("GetCallerIdentity".into())?;
        Ok(self.0.borrow().identity.clone())
    }
}
