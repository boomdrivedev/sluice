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

use crate::instance::Aws;

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
    pub aws: Aws,
    pub files: Option<aws_runtime::env_config::file::EnvConfigFiles>,
}

impl Chain {
    /// The host's chain, as its settings name it.
    pub fn of(aws: &Aws) -> Chain {
        Chain {
            aws: aws.clone(),
            files: None,
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
