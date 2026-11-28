//! The AWS calls a CodeBuild pool makes, through the AWS SDK (record 0119;
//! design D9 of the change `a-codebuild-fleet-runs-a-queues-checks`), as
//! timber's KMS signer reaches KMS (timber record 0017): credentials found
//! as the AWS tools find them at each call and kept by none, the host's
//! `[aws] profile` where it names one, rustls on ring, the SDK's own
//! retries off and its timeouts weir-bounded's, each call under sluice's
//! one retry policy (rule core.29) on a current-thread runtime made for it
//! alone, so every caller stays blocking (ADR 0062).

use aws_config::provider_config::ProviderConfig;
use aws_config::retry::RetryConfig;
use aws_config::timeout::TimeoutConfig;
use aws_config::{BehaviorVersion, ConfigLoader, Region};
use aws_sdk_codebuild::config::SharedHttpClient;
use aws_smithy_http_client::tls::{self, rustls_provider::CryptoMode};

use std::future::Future;
use std::rc::Rc;

use anyhow::{Context as _, Result};
use aws_config::SdkConfig;
use aws_sdk_codebuild as cb;
use aws_sdk_codebuild::config::http::HttpResponse;
use aws_sdk_codebuild::error::{ProvideErrorMetadata, SdkError};
use chrono::{DateTime, Utc};
use sluice_seams::Unanswered;

use super::aws::{Artifact, Aws, Build, Fleet, Project, PublicKey};
use crate::instance;
use crate::retry::{self, Failed, Judged, Kind};

/// The HTTPS client every call reaches AWS through: rustls on ring, the
/// crypto `ureq` already uses, so the build needs no other.
pub fn https() -> SharedHttpClient {
    aws_smithy_http_client::Builder::new()
        .tls_provider(tls::Provider::Rustls(CryptoMode::Ring))
        .build_https()
}

/// How a call finds its credentials: the host's `[aws]`, and, for a test,
/// the AWS configuration files read in place of the host's.
#[derive(Clone, Debug, Default)]
pub struct Chain {
    pub aws: instance::Aws,
    pub files: Option<aws_runtime::env_config::file::EnvConfigFiles>,
    /// For a test, the one address every service is reached at, as a fake
    /// of AWS answers there, with credentials of no one and nothing of the
    /// host's AWS files or environment read.
    pub endpoint: Option<String>,
    /// For a test, the client and credentials every call is made with,
    /// reading nothing of the host's.
    #[cfg(test)]
    pub given: Option<(
        SharedHttpClient,
        aws_sdk_codebuild::config::SharedCredentialsProvider,
    )>,
}

impl Chain {
    /// The host's chain, as its settings name it.
    pub fn of(aws: &instance::Aws) -> Chain {
        Chain {
            aws: aws.clone(),
            ..Chain::default()
        }
    }

    /// For a test: every service reached at `url`, as [`Chain::endpoint`]
    /// says.
    pub fn reaching(url: &str) -> Chain {
        Chain {
            endpoint: Some(url.into()),
            ..Chain::default()
        }
    }

    /// Record 0119 (design D9): the SDK's configuration for a call in
    /// `region`. Where `[aws] profile` names a profile, the call is made as
    /// that profile, its credentials read from it alone, whatever the
    /// environment of the command that started the process names; where it
    /// names none, the SDK's own chain reads the environment, the profile it
    /// names, or a build's role. Its retries are off, since sluice's policy
    /// tries a call again, and each try is bounded by weir-bounded's limit.
    pub fn loader(&self, region: &str) -> ConfigLoader {
        let limit = weir_bounded::TRY_FOR;
        let timeouts = TimeoutConfig::builder()
            .connect_timeout(limit)
            .operation_attempt_timeout(limit)
            .operation_timeout(limit)
            .build();
        let region = Region::new(region.to_string());
        let mut loader = aws_config::defaults(BehaviorVersion::latest())
            .region(region.clone())
            .http_client(https())
            .retry_config(RetryConfig::disabled())
            .timeout_config(timeouts.clone());
        if let Some(files) = &self.files {
            loader = loader.profile_files(files.clone());
        }
        if let Some(url) = &self.endpoint {
            use aws_runtime::env_config::file::EnvConfigFiles;
            let none = EnvConfigFiles::builder()
                .include_default_config_file(false)
                .include_default_credentials_file(false)
                .build();
            return loader
                .profile_files(none)
                .endpoint_url(url)
                .credentials_provider(aws_sdk_codebuild::config::Credentials::new(
                    "AKIATEST", "test", None, None, "a test",
                ));
        }
        #[cfg(test)]
        if let Some((http, credentials)) = &self.given {
            return loader
                .empty_test_environment()
                .http_client(http.clone())
                .credentials_provider(credentials.clone());
        }
        if let Some(profile) = &self.aws.profile {
            let reach = ProviderConfig::default()
                .with_region(Some(region))
                .with_behavior_version(Some(BehaviorVersion::latest()))
                .with_http_client(https())
                .with_retry_config(RetryConfig::disabled())
                .with_timeout_config(timeouts);
            let mut credentials = aws_config::profile::ProfileFileCredentialsProvider::builder()
                .configure(&reach)
                .profile_name(profile);
            if let Some(files) = &self.files {
                credentials = credentials.profile_files(files.clone());
            }
            loader = loader
                .profile_name(profile)
                .credentials_provider(credentials.build());
        }
        loader
    }
}

impl Chain {
    /// The region the chain names, as the AWS tools find it: the
    /// environment's, else the profile's; `None` where it names none.
    pub fn region(&self) -> Option<String> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .ok()?;
        let mut chain = aws_config::default_provider::region::DefaultRegionChain::builder();
        if let Some(profile) = &self.aws.profile {
            chain = chain.profile_name(profile);
        }
        runtime
            .block_on(chain.build().region())
            .map(|r| r.to_string())
    }
}

/// The calls of [`super::aws::Aws`] through the SDK, in one region, each
/// under `policy`.
pub struct Sdk {
    chain: Chain,
    region: String,
    policy: Rc<retry::Policy>,
}

impl Sdk {
    /// The calls in `region`, made as `chain` finds credentials, each under
    /// `policy`.
    pub fn new(chain: Chain, region: &str, policy: Rc<retry::Policy>) -> Sdk {
        Sdk {
            chain,
            region: region.into(),
            policy,
        }
    }

    /// `op` run under the policy as a call of `kind`, each try on the
    /// call's own current-thread runtime with the configuration loaded
    /// afresh, so a credential AWS refused is bought afresh for the next
    /// (rule core.29). A start whose answer was not read fails
    /// [`Unanswered`].
    fn call<T, F>(&self, what: &str, kind: Kind, op: impl Fn(SdkConfig) -> F) -> Result<T>
    where
        F: Future<Output = Result<T, Failed<anyhow::Error>>>,
    {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("making a runtime for a call to AWS")?;
        let made = self.policy.run(kind, |_| {
            let config = runtime.block_on(self.chain.loader(&self.region).load());
            runtime.block_on(op(config))
        });
        drop(runtime);
        match made {
            Ok(v) => Ok(v),
            Err(stopped) if kind == Kind::Write && stopped.judged == Judged::Unknown => {
                Err(anyhow::Error::new(Unanswered(format!(
                    "{what}: {:#}; whether AWS made it is read before anything is made again",
                    stopped.error
                ))))
            }
            Err(stopped) => Err(stopped.into_error().context(what.to_string())),
        }
    }
}

/// The codes AWS answers a call whose credential it refused, before it
/// judged the call: tried again on a credential bought afresh.
const REFUSED_CREDENTIALS: [&str; 6] = [
    "ExpiredToken",
    "ExpiredTokenException",
    "InvalidClientTokenId",
    "UnrecognizedClientException",
    "InvalidSignatureException",
    "RequestExpired",
];

/// The codes AWS answers a call it would not judge just then.
const THROTTLED: [&str; 6] = [
    "Throttling",
    "ThrottlingException",
    "ThrottledException",
    "TooManyRequestsException",
    "RequestLimitExceeded",
    "SlowDown",
];

/// How a call that failed stands, from what the SDK answered: a call never
/// sent, or throttled, AWS never judged; a refused credential is tried on a
/// fresh one; a 5xx, a timeout or an answer that could not be read is not
/// known to have been judged; anything else AWS answered.
fn failed<E>(error: SdkError<E, HttpResponse>) -> Failed<anyhow::Error>
where
    E: ProvideErrorMetadata + std::error::Error + Send + Sync + 'static,
{
    let judged = match &error {
        SdkError::ConstructionFailure(_) => Judged::Answered,
        SdkError::TimeoutError(_) | SdkError::ResponseError(_) => Judged::Unknown,
        SdkError::DispatchFailure(d) if d.is_io() || d.is_timeout() => Judged::Unknown,
        SdkError::DispatchFailure(_) => Judged::Never,
        SdkError::ServiceError(s) => {
            let status = s.raw().status().as_u16();
            match error.code() {
                Some(c) if REFUSED_CREDENTIALS.contains(&c) => Judged::Credential,
                Some(c) if THROTTLED.contains(&c) => Judged::Never,
                _ if status == 429 => Judged::Never,
                _ if status >= 500 => Judged::Unknown,
                _ => Judged::Answered,
            }
        }
        _ => Judged::Unknown,
    };
    let words = match (error.code(), error.message()) {
        (Some(code), Some(message)) => format!("{code}: {message}"),
        (Some(code), None) => code.to_string(),
        _ => words(&error),
    };
    Failed::new(judged, anyhow::anyhow!(words))
}

/// An error's words and its sources' after them, each once.
fn words(error: &(dyn std::error::Error + 'static)) -> String {
    let mut said: Vec<String> = Vec::new();
    let mut at = Some(error);
    while let Some(e) = at {
        let w = e.to_string();
        if said.last() != Some(&w) {
            said.push(w);
        }
        at = e.source();
    }
    said.join(": ")
}

/// A time AWS answered, as chrono reads one.
fn time(t: Option<&cb::primitives::DateTime>) -> Option<DateTime<Utc>> {
    t.and_then(|t| DateTime::from_timestamp(t.secs(), t.subsec_nanos()))
}

/// A build as the profile reads it.
fn build_of(b: &cb::types::Build) -> Build {
    Build {
        id: b.id().unwrap_or_default().into(),
        arn: b.arn().unwrap_or_default().into(),
        project: b.project_name().unwrap_or_default().into(),
        status: b
            .build_status()
            .map(|s| s.as_str().to_string())
            .unwrap_or_default(),
        phase: b.current_phase().unwrap_or_default().into(),
        complete: b.build_complete(),
        start: time(b.start_time()),
        end: time(b.end_time()),
        queued_end: b
            .phases()
            .iter()
            .find(|p| p.phase_type() == Some(&cb::types::BuildPhaseType::Queued))
            .and_then(|p| time(p.end_time())),
        variables: b
            .environment()
            .map(|e| {
                e.environment_variables()
                    .iter()
                    .map(|v| (v.name().to_string(), v.value().to_string()))
                    .collect()
            })
            .unwrap_or_default(),
        artifact: b.artifacts().and_then(|a| {
            a.location().map(|l| Artifact {
                location: l.into(),
                sha256: a.sha256_sum().map(String::from),
            })
        }),
    }
}

/// An S3 client for `config`, addressing a bucket by path where an endpoint
/// in place of AWS's is named, as one standing for S3 serves it.
fn s3_client(config: &SdkConfig) -> aws_sdk_s3::Client {
    let named = config.endpoint_url().is_some()
        || std::env::var_os("AWS_ENDPOINT_URL_S3").is_some()
        || std::env::var_os("AWS_ENDPOINT_URL").is_some();
    aws_sdk_s3::Client::from_conf(
        aws_sdk_s3::config::Builder::from(config)
            .force_path_style(named)
            .build(),
    )
}

impl Aws for Sdk {
    /// 88, record 0122 (design D3): a start, which is made at most once
    /// under the policy, its idempotency token the claim so CodeBuild
    /// starts no second build for a start sent again within its window.
    fn start_build(
        &self,
        project: &str,
        variables: &[(String, String)],
        token: &str,
    ) -> Result<Build> {
        let overrides: Vec<cb::types::EnvironmentVariable> = variables
            .iter()
            .map(|(n, v)| {
                cb::types::EnvironmentVariable::builder()
                    .name(n)
                    .value(v)
                    .r#type(cb::types::EnvironmentVariableType::Plaintext)
                    .build()
                    .context("a build's variable")
            })
            .collect::<Result<_>>()?;
        self.call(&format!("StartBuild {project}"), Kind::Write, |config| {
            let overrides = overrides.clone();
            async move {
                let started = cb::Client::new(&config)
                    .start_build()
                    .project_name(project)
                    .set_environment_variables_override(Some(overrides))
                    .idempotency_token(token)
                    .send()
                    .await
                    .map_err(failed)?;
                started.build_value().map(build_of).ok_or_else(|| {
                    Failed::new(
                        Judged::Unknown,
                        anyhow::anyhow!("CodeBuild answered the start with no build"),
                    )
                })
            }
        })
    }

    fn list_builds_for_project(&self, project: &str) -> Result<Vec<String>> {
        self.call(
            &format!("ListBuildsForProject {project}"),
            Kind::Read,
            |config| async move {
                let listed = cb::Client::new(&config)
                    .list_builds_for_project()
                    .project_name(project)
                    .sort_order(cb::types::SortOrderType::Descending)
                    .send()
                    .await
                    .map_err(failed)?;
                Ok(listed.ids().to_vec())
            },
        )
    }

    fn batch_get_builds(&self, ids: &[String]) -> Result<Vec<Build>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        self.call("BatchGetBuilds", Kind::Read, |config| async move {
            let got = cb::Client::new(&config)
                .batch_get_builds()
                .set_ids(Some(ids.to_vec()))
                .send()
                .await
                .map_err(failed)?;
            Ok(got.builds().iter().map(build_of).collect())
        })
    }

    fn batch_get_projects(&self, names: &[String]) -> Result<Vec<Project>> {
        self.call("BatchGetProjects", Kind::Read, |config| async move {
            let got = cb::Client::new(&config)
                .batch_get_projects()
                .set_names(Some(names.to_vec()))
                .send()
                .await
                .map_err(failed)?;
            Ok(got
                .projects()
                .iter()
                .map(|p| Project {
                    name: p.name().unwrap_or_default().into(),
                    arn: p.arn().unwrap_or_default().into(),
                    environment: p
                        .environment()
                        .map(|e| e.r#type().as_str().to_string())
                        .unwrap_or_default(),
                    fleet: p
                        .environment()
                        .and_then(|e| e.fleet())
                        .and_then(|f| f.fleet_arn())
                        .map(String::from),
                    source: p
                        .source()
                        .map(|s| s.r#type().as_str().to_string())
                        .unwrap_or_default(),
                    buildspec: p.source().and_then(|s| s.buildspec()).map(String::from),
                    timeout_minutes: p.timeout_in_minutes(),
                    service_role: p.service_role().map(String::from),
                    artifacts: p
                        .artifacts()
                        .map(|a| {
                            (
                                a.r#type().as_str().to_string(),
                                a.packaging().map(|k| k.as_str().to_string()),
                            )
                        })
                        .unwrap_or_default(),
                })
                .collect())
        })
    }

    fn batch_get_fleets(&self, arns: &[String]) -> Result<Vec<Fleet>> {
        self.call("BatchGetFleets", Kind::Read, |config| async move {
            let got = cb::Client::new(&config)
                .batch_get_fleets()
                .set_names(Some(arns.to_vec()))
                .send()
                .await
                .map_err(failed)?;
            Ok(got
                .fleets()
                .iter()
                .map(|f| Fleet {
                    arn: f.arn().unwrap_or_default().into(),
                    name: f.name().unwrap_or_default().into(),
                    environment: f
                        .environment_type()
                        .map(|e| e.as_str().to_string())
                        .unwrap_or_default(),
                    base_capacity: f.base_capacity(),
                    max_capacity: f.scaling_configuration().and_then(|s| s.max_capacity()),
                })
                .collect())
        })
    }

    fn get_object(&self, bucket: &str, key: &str) -> Result<Vec<u8>> {
        self.call(
            &format!("GetObject s3://{bucket}/{key}"),
            Kind::Read,
            |config| async move {
                let got = s3_client(&config)
                    .get_object()
                    .bucket(bucket)
                    .key(key)
                    .send()
                    .await
                    .map_err(failed)?;
                let body = got.body.collect().await.map_err(|e| {
                    Failed::new(Judged::Unknown, anyhow::anyhow!("reading the object: {e}"))
                })?;
                Ok(body.into_bytes().to_vec())
            },
        )
    }

    /// timber record 0017 (design D7): `ECDSA_SHA_256` over `digest`, sent
    /// as a digest, so nothing of the statement leaves the host.
    fn sign(&self, key: &str, digest: &[u8; 32]) -> Result<Vec<u8>> {
        self.call(&format!("Sign {key}"), Kind::Read, |config| async move {
            let signed = aws_sdk_kms::Client::new(&config)
                .sign()
                .key_id(key)
                .message(aws_sdk_kms::primitives::Blob::new(digest.to_vec()))
                .message_type(aws_sdk_kms::types::MessageType::Digest)
                .signing_algorithm(aws_sdk_kms::types::SigningAlgorithmSpec::EcdsaSha256)
                .send()
                .await
                .map_err(failed)?;
            if signed.key_id() != Some(key) {
                return Err(Failed::new(
                    Judged::Answered,
                    anyhow::anyhow!(
                        "KMS answered the signature as made by the key {}, not by {key}",
                        signed.key_id().unwrap_or("none")
                    ),
                ));
            }
            signed
                .signature()
                .map(|s| s.as_ref().to_vec())
                .ok_or_else(|| {
                    Failed::new(
                        Judged::Answered,
                        anyhow::anyhow!("KMS answered no signature"),
                    )
                })
        })
    }

    fn get_public_key(&self, key: &str) -> Result<PublicKey> {
        self.call(
            &format!("GetPublicKey {key}"),
            Kind::Read,
            |config| async move {
                let got = aws_sdk_kms::Client::new(&config)
                    .get_public_key()
                    .key_id(key)
                    .send()
                    .await
                    .map_err(failed)?;
                Ok(PublicKey {
                    arn: got.key_id().unwrap_or_default().into(),
                    spec: got
                        .key_spec()
                        .map(|s| s.as_str().to_string())
                        .unwrap_or_default(),
                    usage: got
                        .key_usage()
                        .map(|u| u.as_str().to_string())
                        .unwrap_or_default(),
                    der: got
                        .public_key()
                        .map(|k| k.as_ref().to_vec())
                        .unwrap_or_default(),
                })
            },
        )
    }

    fn caller_identity(&self) -> Result<String> {
        self.call("GetCallerIdentity", Kind::Read, |config| async move {
            let got = aws_sdk_sts::Client::new(&config)
                .get_caller_identity()
                .send()
                .await
                .map_err(failed)?;
            Ok(got.arn().unwrap_or_default().to_string())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_codebuild::config::{Credentials, SharedCredentialsProvider};
    use aws_smithy_http_client::test_util::{ReplayEvent, StaticReplayClient};
    use aws_smithy_types::body::SdkBody;

    /// An answer of CodeBuild's, `status` with `body`, to any request.
    fn answer(status: u16, body: &str) -> ReplayEvent {
        ReplayEvent::new(
            http::Request::builder()
                .uri("https://codebuild.us-east-1.amazonaws.com/")
                .body(SdkBody::empty())
                .unwrap(),
            http::Response::builder()
                .status(status)
                .header("content-type", "application/x-amz-json-1.1")
                .body(SdkBody::from(body))
                .unwrap(),
        )
    }

    /// An error CodeBuild answers, by its code.
    fn error(status: u16, code: &str) -> ReplayEvent {
        answer(
            status,
            &format!("{{\"__type\":\"{code}\",\"message\":\"{code} said\"}}"),
        )
    }

    /// The calls in us-east-1 through `replay`, with credentials of no one,
    /// under a policy whose pauses are skipped.
    fn sdk(replay: &StaticReplayClient) -> Sdk {
        let chain = Chain {
            given: Some((
                SharedHttpClient::new(replay.clone()),
                SharedCredentialsProvider::new(Credentials::new(
                    "AKIA", "secret", None, None, "test",
                )),
            )),
            ..Chain::default()
        };
        Sdk::new(
            chain,
            "us-east-1",
            Rc::new(retry::Policy::command().recording()),
        )
    }

    /// Rule core.29 (design D9): a read AWS throttled, or answered a 5xx
    /// to, is tried again under the policy and its answer read; a read AWS
    /// answered is not tried again, failing in AWS's words.
    #[test]
    fn a_read_aws_did_not_judge_is_tried_again_under_the_policy() {
        let replay = StaticReplayClient::new(vec![
            error(400, "ThrottlingException"),
            error(500, "InternalFailure"),
            answer(
                200,
                r#"{"ids":["sluice-run-linux-x64:b","sluice-run-linux-x64:a"]}"#,
            ),
        ]);
        let ids = sdk(&replay)
            .list_builds_for_project("sluice-run-linux-x64")
            .unwrap();
        assert_eq!(ids, ["sluice-run-linux-x64:b", "sluice-run-linux-x64:a"]);
        assert_eq!(replay.actual_requests().count(), 3);

        let replay = StaticReplayClient::new(vec![
            error(400, "ResourceNotFoundException"),
            answer(200, r#"{"ids":[]}"#),
        ]);
        let e = format!(
            "{:#}",
            sdk(&replay)
                .list_builds_for_project("sluice-run-linux-x64")
                .unwrap_err()
        );
        assert!(
            e.contains("ListBuildsForProject sluice-run-linux-x64")
                && e.contains("ResourceNotFoundException: ResourceNotFoundException said"),
            "{e}"
        );
        assert_eq!(replay.actual_requests().count(), 1);
    }

    /// Rule core.29, record 0122 (design D3): a start whose answer was not
    /// read is not sent again, failing `Unanswered` for its caller to settle
    /// by reading; one AWS throttled was never judged and is sent again.
    #[test]
    fn a_start_whose_answer_was_lost_is_not_sent_again() {
        let replay = StaticReplayClient::new(vec![
            error(500, "InternalFailure"),
            answer(200, r#"{"build":{"id":"sluice-run-linux-x64:a"}}"#),
        ]);
        let variables = [("SLUICE_CLAIM".to_string(), "pool/codebuild/4".to_string())];
        let e = sdk(&replay)
            .start_build("sluice-run-linux-x64", &variables, "pool/codebuild/4")
            .unwrap_err();
        assert!(sluice_seams::unanswered(&e), "{e:#}");
        assert!(
            format!("{e:#}").contains("StartBuild sluice-run-linux-x64"),
            "{e:#}"
        );
        assert_eq!(replay.actual_requests().count(), 1);

        let replay = StaticReplayClient::new(vec![
            error(400, "ThrottlingException"),
            answer(
                200,
                r#"{"build":{"id":"sluice-run-linux-x64:a","buildStatus":"IN_PROGRESS","currentPhase":"SUBMITTED","environment":{"type":"LINUX_EC2","image":"i","computeType":"BUILD_GENERAL1_SMALL","environmentVariables":[{"name":"SLUICE_CLAIM","value":"pool/codebuild/4","type":"PLAINTEXT"}]}}}"#,
            ),
        ]);
        let started = sdk(&replay)
            .start_build("sluice-run-linux-x64", &variables, "pool/codebuild/4")
            .unwrap();
        assert_eq!(started.id, "sluice-run-linux-x64:a");
        assert_eq!(started.variables["SLUICE_CLAIM"], "pool/codebuild/4");
        assert_eq!(replay.actual_requests().count(), 2);
        let sent: Vec<String> = replay
            .actual_requests()
            .map(|r| String::from_utf8_lossy(r.body().bytes().unwrap()).to_string())
            .collect();
        assert!(
            sent.iter()
                .all(|b| b.contains("\"idempotencyToken\":\"pool/codebuild/4\"")),
            "{sent:?}"
        );
    }
}
