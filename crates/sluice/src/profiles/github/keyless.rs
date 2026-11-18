//! A GitHub Actions job's keyless signature, verified on any host from the
//! records alone (78, 79, 141; record 0119; design D13 of the change
//! `a-runs-life-is-written-first-and-actions-runs-its-checks`).
//!
//! A pool's sign job signs its run's statement with the job's own identity,
//! a short-lived certificate for the workflow and ref it ran from; the
//! bundle it records holds the statement, the certificate and the
//! transparency log's entry or a timestamp. A host verifies it against the
//! trusted roots sluice carries, the public-good instance's for a bundle
//! with a log entry and GitHub's for one with a timestamp and none, calling
//! no service: a root GitHub rotates after sluice's release leaves new
//! bundles unverified, missing and never a pass, until the next release
//! carries it. The certificate counts only when its identity is the
//! signer's: `https://github.com/<owner>/<repo>/<workflow>@<ref>`, issued
//! by GitHub's token service.

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine as _;
use serde_json::Value;
use sigstore_trust_root::{SigstoreInstance, TrustedRoot};
use sigstore_verify::types::{Bundle, Sha256Hash, SignatureContent};
use sigstore_verify::{verify as verify_bundle, VerificationPolicy};

/// The issuer of a GitHub Actions job's identity token.
pub const GITHUB_ISSUER: &str = "https://token.actions.githubusercontent.com";

/// The in-toto payload type a statement is signed under.
const IN_TOTO: &str = "application/vnd.in-toto+json";

/// Whose signature a bundle must bear: the certificate's subject, a URI,
/// and its issuer, where one is asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub uri: String,
    pub issuer: Option<String>,
}

impl Identity {
    /// A GitHub Actions workflow's identity: `workflow`, a path in the
    /// repository, run from `git_ref` of `owner/repo`, issued by GitHub's
    /// token service.
    pub fn workflow(owner: &str, repo: &str, workflow: &str, git_ref: &str) -> Identity {
        Identity {
            uri: format!("https://github.com/{owner}/{repo}/{workflow}@{git_ref}"),
            issuer: Some(GITHUB_ISSUER.into()),
        }
    }
}

/// The in-toto statement a bundle's DSSE envelope holds, as signed, read
/// without verifying anything.
pub fn statement_of(bundle: &str) -> Result<Value> {
    let v: Value = serde_json::from_str(bundle).context("reading a bundle")?;
    let envelope = &v["dsseEnvelope"];
    if !envelope.is_object() {
        bail!("the bundle holds no DSSE envelope");
    }
    let kind = envelope["payloadType"].as_str().unwrap_or_default();
    if kind != IN_TOTO {
        bail!("the bundle's envelope holds {kind:?}, not an in-toto statement");
    }
    let payload = envelope["payload"]
        .as_str()
        .ok_or_else(|| anyhow!("the bundle's envelope holds no payload"))?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload)
        .context("decoding the bundle's payload")?;
    serde_json::from_slice(&bytes).context("reading the bundle's statement")
}

/// 78, 79, record 0119 (design D13): `bundle` verified against the roots
/// sluice carries, its certificate's identity `identity`: its statement,
/// where it verifies; otherwise why it counts for nothing, naming the
/// identity it carried. A bundle with a transparency log entry is verified
/// against the public-good instance's root, one with none, timestamped,
/// against GitHub's.
pub fn verify(bundle: &str, identity: &Identity) -> Result<Value> {
    let statement = statement_of(bundle)?;
    let parsed = Bundle::from_json(bundle).map_err(|e| anyhow!("reading a bundle: {e}"))?;
    if !matches!(parsed.content, SignatureContent::DsseEnvelope(_)) {
        bail!("the bundle holds no DSSE envelope");
    }
    // The statement binds itself: its first subject with a SHA-256 digest
    // is the artifact the signature is checked against.
    let digest = statement["subject"]
        .as_array()
        .into_iter()
        .flatten()
        .find_map(|s| s["digest"]["sha256"].as_str())
        .ok_or_else(|| anyhow!("the bundle's statement names no subject by SHA-256"))?;
    let digest = Sha256Hash::from_hex(digest).map_err(|e| anyhow!("{digest}: {e}"))?;
    let logged = !parsed.verification_material.tlog_entries.is_empty();
    let (instance, name) = match logged {
        true => (SigstoreInstance::PublicGood, "the public-good instance"),
        false => (SigstoreInstance::GitHub, "GitHub's instance"),
    };
    let root =
        TrustedRoot::from_embedded(instance).map_err(|e| anyhow!("reading {name}'s root: {e}"))?;
    let mut policy = VerificationPolicy::any_identity().require_identity(identity.uri.as_str());
    if let Some(issuer) = &identity.issuer {
        policy = policy.require_issuer(issuer.as_str());
    }
    if !logged {
        // GitHub's instance keeps no transparency log and gives no
        // certificate timestamps: its RFC 3161 timestamp stands for them.
        policy = policy.skip_tlog_unsafe().skip_sct();
    }
    match verify_bundle(digest, &parsed, &policy, &root) {
        Ok(_) => Ok(statement),
        Err(e) => bail!(
            "a bundle whose certificate carries {} does not verify against {name}'s root \
             for {}: {e}",
            carried(&parsed).unwrap_or_else(|| "no identity".into()),
            identity.uri
        ),
    }
}

/// The identity a bundle's certificate carries, as its subject names it,
/// for a refusal's words.
fn carried(bundle: &Bundle) -> Option<String> {
    let any = VerificationPolicy::any_identity()
        .skip_tlog_unsafe()
        .skip_sct()
        .skip_certificate_chain();
    for instance in [SigstoreInstance::PublicGood, SigstoreInstance::GitHub] {
        let root = TrustedRoot::from_embedded(instance).ok()?;
        let SignatureContent::DsseEnvelope(envelope) = &bundle.content else {
            return None;
        };
        let statement: Value = serde_json::from_slice(envelope.payload.as_bytes()).ok()?;
        let digest = statement["subject"]
            .as_array()?
            .iter()
            .find_map(|s| s["digest"]["sha256"].as_str())?;
        let digest = Sha256Hash::from_hex(digest).ok()?;
        if let Ok(r) = verify_bundle(digest, bundle, &any, &root) {
            return r.identity().map(|i| format!("{i:?}"));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bundle whose envelope holds `statement`, signed by no one: what a
    /// statement is read from, before anything is verified.
    fn bundle_holding(statement: &Value) -> String {
        let payload = base64::engine::general_purpose::STANDARD.encode(statement.to_string());
        serde_json::json!({
            "mediaType": "application/vnd.dev.sigstore.bundle.v0.3+json",
            "verificationMaterial": {},
            "dsseEnvelope": {
                "payload": payload,
                "payloadType": IN_TOTO,
                "signatures": [{ "sig": "" }],
            },
        })
        .to_string()
    }

    /// Record 0119 (design D13): a keyless bundle's envelope holds the
    /// run's statement as sluice signs it, read back whole; an envelope of
    /// anything but an in-toto statement holds none.
    #[test]
    fn a_keyless_bundle_holds_the_runs_statement() {
        let statement = serde_json::json!({
            "_type": "https://in-toto.io/Statement/v1",
            "subject": [{
                "name": "a:test",
                "digest": { "sha256": "54303491a8418fbed24344b513546182c29b43bf282ceb433af65e2299f9271f" },
            }],
            "predicateType": "https://agentplot.github.io/sluice/run/v1",
            "predicate": { "claim": "pool/forge/7", "combination": "c0ffee" },
        });
        assert_eq!(
            statement_of(&bundle_holding(&statement)).unwrap(),
            statement
        );
        let other = bundle_holding(&statement).replace(IN_TOTO, "text/plain");
        assert!(statement_of(&other).is_err());
        assert!(statement_of("{\"messageSignature\": {}}").is_err());
        assert_eq!(
            Identity::workflow("o", "r", ".github/workflows/sluice.yml", "refs/heads/main").uri,
            "https://github.com/o/r/.github/workflows/sluice.yml@refs/heads/main"
        );
    }
}
