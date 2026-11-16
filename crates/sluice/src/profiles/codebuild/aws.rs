//! Every call a CodeBuild pool makes to AWS, behind one trait (record 0119;
//! design D9 of the change `a-codebuild-fleet-runs-a-queues-checks`): the
//! SDK's in [`super::sdk`], and a fake's in [`super::fake`] for tests, so
//! no test reaches AWS. What each answers is only what the profile reads.

use std::collections::BTreeMap;

use anyhow::Result;
use chrono::{DateTime, Utc};

/// A build as `BatchGetBuilds` answers it, as far as the profile reads it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Build {
    /// `<project>:<uuid>`.
    pub id: String,
    pub arn: String,
    pub project: String,
    /// `IN_PROGRESS`, `SUCCEEDED`, `FAILED`, `FAULT`, `TIMED_OUT` or
    /// `STOPPED`.
    pub status: String,
    /// The phase it is in: `SUBMITTED`, `QUEUED`, `PROVISIONING`, …,
    /// `COMPLETED`.
    pub phase: String,
    /// Whether it has ended.
    pub complete: bool,
    pub start: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
    /// When its `QUEUED` phase ended, where it has: when an instance took
    /// it.
    pub queued_end: Option<DateTime<Utc>>,
    /// Its environment's variables, the overrides it was started with
    /// among them, by name.
    pub variables: BTreeMap<String, String>,
    /// Its artifact, where it uploaded one.
    pub artifact: Option<Artifact>,
}

impl Build {
    /// Whether it ended in one of the states that start a sign build:
    /// `SUCCEEDED`, `FAILED`, `FAULT` or `TIMED_OUT`, every end but a stop
    /// (record 0119; design D2).
    pub fn ended_unstopped(&self) -> bool {
        self.complete && self.status != "STOPPED" && self.status != "IN_PROGRESS"
    }
}

/// A build's artifact: where CodeBuild says it put it, an S3 ARN
/// `arn:aws:s3:::<bucket>/<key>`, and the SHA-256 it recorded of it, hex,
/// which it records for a `ZIP` (design D6).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Artifact {
    pub location: String,
    pub sha256: Option<String>,
}

impl Artifact {
    /// The bucket and key `location` names.
    pub fn object(&self) -> Option<(String, String)> {
        let rest = self.location.strip_prefix("arn:aws:s3:::").or_else(|| {
            // Another partition's: `arn:<partition>:s3:::`.
            let mut parts = self.location.splitn(6, ':');
            match (parts.next(), parts.next(), parts.next()) {
                (Some("arn"), Some(_), Some("s3")) => {
                    let _ = (parts.next(), parts.next());
                    parts.next()
                }
                _ => None,
            }
        })?;
        let (bucket, key) = rest.split_once('/')?;
        (!bucket.is_empty() && !key.is_empty()).then(|| (bucket.to_string(), key.to_string()))
    }
}

/// A project as `BatchGetProjects` answers it, as far as `sluice pool
/// check` reads it (design D11).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Project {
    pub name: String,
    pub arn: String,
    /// `LINUX_EC2`, `ARM_EC2`, `LINUX_CONTAINER`, …
    pub environment: String,
    /// The fleet it runs on, by its ARN, where it names one.
    pub fleet: Option<String>,
    /// Its source's type, `NO_SOURCE` for a project with none.
    pub source: String,
    /// Its buildspec, inline.
    pub buildspec: Option<String>,
    pub timeout_minutes: Option<i32>,
    pub service_role: Option<String>,
    /// Its artifacts' type and packaging: `S3` and `ZIP` for a run project.
    pub artifacts: (String, Option<String>),
}

/// A fleet as `BatchGetFleets` answers it (design D11).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fleet {
    pub arn: String,
    pub name: String,
    /// `LINUX_EC2`, `ARM_EC2`, …
    pub environment: String,
    pub base_capacity: Option<i32>,
    /// The most instances its scaling grows it to, where it scales.
    pub max_capacity: Option<i32>,
}

/// A key's public half as KMS's `GetPublicKey` answers it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PublicKey {
    /// The key's ARN, which KMS answers whatever the key was asked by.
    pub arn: String,
    /// `ECC_NIST_P256`, …
    pub spec: String,
    /// `SIGN_VERIFY`, …
    pub usage: String,
    /// Its DER SubjectPublicKeyInfo.
    pub der: Vec<u8>,
}

/// Every call the profile makes, each under sluice's one retry policy in
/// the SDK's (rule core.29). A read AWS refuses, or cannot answer, fails
/// in AWS's words; a start whose answer was not read fails
/// [`sluice_seams::Unanswered`], never sent again, for its caller to
/// settle by reading.
pub trait Aws {
    /// `StartBuild` of `project` with `variables` overridden, and nothing
    /// else, under `token`, CodeBuild's idempotency token.
    fn start_build(
        &self,
        project: &str,
        variables: &[(String, String)],
        token: &str,
    ) -> Result<Build>;
    /// `ListBuildsForProject`, newest first, its first page.
    fn list_builds_for_project(&self, project: &str) -> Result<Vec<String>>;
    /// `BatchGetBuilds` of `ids`, by id or ARN.
    fn batch_get_builds(&self, ids: &[String]) -> Result<Vec<Build>>;
    fn batch_get_projects(&self, names: &[String]) -> Result<Vec<Project>>;
    fn batch_get_fleets(&self, arns: &[String]) -> Result<Vec<Fleet>>;
    /// `GetObject`'s body.
    fn get_object(&self, bucket: &str, key: &str) -> Result<Vec<u8>>;
    /// `Sign` of `digest` with `key`, `ECDSA_SHA_256` as `DIGEST`: the
    /// signature's DER bytes as KMS answered them.
    fn sign(&self, key: &str, digest: &[u8; 32]) -> Result<Vec<u8>>;
    /// `GetPublicKey` of `key`, by its ARN or an alias.
    fn get_public_key(&self, key: &str) -> Result<PublicKey>;
    /// `GetCallerIdentity`'s ARN: who the calls are made as.
    fn caller_identity(&self) -> Result<String>;
}
