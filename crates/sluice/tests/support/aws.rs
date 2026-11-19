//! AWS as a CodeBuild pool's commands reach it, on [`super::http::Fake`]:
//! CodeBuild, S3, KMS and STS answering one address as each answers,
//! holding builds, projects, fleets, objects and a P-256 key the test
//! holds, so no test reaches AWS (design D9 of the change
//! `a-codebuild-fleet-runs-a-queues-checks`).
//!
//! A command reaches it through the AWS SDK's own chain, the process's
//! environment pointing every service at it ([`FakeAws::point_here`]),
//! which a test sets only in a child of its own binary it runs alone in.

use std::sync::{Arc, Mutex};

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use p256::ecdsa::signature::hazmat::PrehashSigner as _;
use p256::ecdsa::{Signature, SigningKey};
use serde_json::{json, Value};

use super::http::{self, Answer, Fake, Request};

pub const ACCOUNT: &str = "111122223333";
pub const REGION: &str = "us-east-1";
pub const KEY: &str = "arn:aws:kms:us-east-1:111122223333:key/1234abcd-12ab-34cd-56ef-1234567890ab";

/// What the fake holds.
#[derive(Default)]
pub struct Held {
    /// Each build, as `BatchGetBuilds` answers it, in the order made.
    pub builds: Vec<Value>,
    /// Each project, as `BatchGetProjects` answers it.
    pub projects: Vec<Value>,
    /// Each fleet, as `BatchGetFleets` answers it.
    pub fleets: Vec<Value>,
    /// Each object, by `<bucket>/<key>`.
    pub objects: Vec<(String, Vec<u8>)>,
    /// The key and its alias, where the fake holds one.
    pub key: Option<(String, SigningKey)>,
    /// Who the calls are made as.
    pub identity: String,
    /// The key's spec, where not `ECC_NIST_P256`.
    pub spec: Option<String>,
}

#[derive(Clone)]
pub struct FakeAws {
    pub held: Arc<Mutex<Held>>,
    fake: Arc<Fake>,
}

impl FakeAws {
    pub fn start() -> FakeAws {
        let held = Arc::new(Mutex::new(Held {
            identity: format!("arn:aws:sts::{ACCOUNT}:assumed-role/sluice-start/host"),
            ..Held::default()
        }));
        let at = held.clone();
        let fake = Fake::start(move |r| answer(&at, r));
        FakeAws {
            held,
            fake: Arc::new(fake),
        }
    }

    /// The fake holding a P-256 key aliased `alias`; the key.
    pub fn with_key(&self, alias: &str) -> SigningKey {
        let key = SigningKey::random(&mut rand_core::OsRng);
        self.held.lock().unwrap().key = Some((alias.into(), key.clone()));
        key
    }

    /// Every request it saw.
    pub fn requests(&self) -> Vec<Request> {
        self.fake.requests()
    }

    /// The operations asked of it, by CodeBuild's, KMS's and STS's target
    /// or S3's method and path.
    pub fn operations(&self) -> Vec<String> {
        self.requests().iter().map(operation).collect()
    }

    /// The process's environment pointed at the fake: every service's
    /// endpoint, credentials of no one, and no AWS file read. Only in a
    /// child a test runs alone in.
    pub fn point_here(&self) {
        let url = self.fake.url();
        for service in ["CODEBUILD", "S3", "KMS", "STS"] {
            std::env::set_var(format!("AWS_ENDPOINT_URL_{service}"), url);
        }
        std::env::set_var("AWS_ACCESS_KEY_ID", "AKIATEST");
        std::env::set_var("AWS_SECRET_ACCESS_KEY", "test");
        std::env::set_var("AWS_REGION", REGION);
        std::env::set_var("AWS_CONFIG_FILE", "/dev/null");
        std::env::set_var("AWS_SHARED_CREDENTIALS_FILE", "/dev/null");
        std::env::set_var("AWS_EC2_METADATA_DISABLED", "true");
        for v in ["AWS_PROFILE", "AWS_SESSION_TOKEN", "AWS_ENDPOINT_URL"] {
            std::env::remove_var(v);
        }
    }

    /// A build of `project` the fake holds, ended `status`, its variables
    /// `variables` and its artifact `object` with the SHA-256 `sha256`.
    pub fn put_build(
        &self,
        project: &str,
        status: &str,
        variables: &[(&str, &str)],
        artifact: Option<(&str, &str)>,
    ) -> String {
        let mut held = self.held.lock().unwrap();
        let id = format!("{project}:{:08}", held.builds.len() + 1);
        let arn = format!("arn:aws:codebuild:{REGION}:{ACCOUNT}:build/{id}");
        let mut build = json!({
            "id": id,
            "arn": arn,
            "projectName": project,
            "buildStatus": status,
            "currentPhase": if status == "IN_PROGRESS" { "BUILD" } else { "COMPLETED" },
            "buildComplete": status != "IN_PROGRESS",
            "startTime": 1_794_000_000.0,
            "endTime": 1_794_000_600.0,
            "environment": {
                "type": "LINUX_EC2",
                "image": "aws/codebuild/amazonlinux-x86_64-standard:5.0",
                "computeType": "BUILD_GENERAL1_SMALL",
                "environmentVariables": variables
                    .iter()
                    .map(|(n, v)| json!({ "name": n, "value": v, "type": "PLAINTEXT" }))
                    .collect::<Vec<_>>(),
            },
        });
        if let Some((location, sha256)) = artifact {
            build["artifacts"] = json!({ "location": location, "sha256sum": sha256 });
        }
        held.builds.push(build);
        arn
    }

    /// The stack `stack` deployed for `repository` by this release, its
    /// fleet for linux-x64 growing to `most` instances: its run and sign
    /// projects and its fleet, as CodeBuild answers them.
    pub fn deploy(&self, stack: &str, repository: &str, most: i32) {
        use sluice::profiles::codebuild::template::{buildspecs, System};
        let x64 = System::linux("linux-x64", "linux", "x86_64").unwrap();
        let (run, sign) = buildspecs(sluice::VERSION, repository, &x64);
        let fleet = format!("arn:aws:codebuild:{REGION}:{ACCOUNT}:fleet/{stack}-linux-x64:1");
        let project = |name: &str, environment: Value, buildspec: &str, timeout: i32| {
            json!({
                "name": name,
                "arn": format!("arn:aws:codebuild:{REGION}:{ACCOUNT}:project/{name}"),
                "source": { "type": "NO_SOURCE", "buildspec": buildspec },
                "environment": environment,
                "timeoutInMinutes": timeout,
                "serviceRole": format!("arn:aws:iam::{ACCOUNT}:role/{name}"),
                "artifacts": { "type": "S3", "packaging": "ZIP", "location": "hand-over" },
            })
        };
        let mut held = self.held.lock().unwrap();
        held.projects.push(project(
            &format!("{stack}-run-linux-x64"),
            json!({
                "type": "LINUX_EC2",
                "image": "aws/codebuild/ami/amazonlinux-x86_64-base:latest",
                "computeType": "BUILD_GENERAL1_MEDIUM",
                "fleet": { "fleetArn": fleet },
            }),
            &run,
            90,
        ));
        held.projects.push(project(
            &format!("{stack}-sign"),
            json!({
                "type": "LINUX_CONTAINER",
                "image": "aws/codebuild/amazonlinux-x86_64-standard:5.0",
                "computeType": "BUILD_GENERAL1_SMALL",
            }),
            &sign,
            20,
        ));
        held.fleets.push(json!({
            "arn": fleet,
            "name": format!("{stack}-linux-x64"),
            "environmentType": "LINUX_EC2",
            "computeType": "BUILD_GENERAL1_MEDIUM",
            "baseCapacity": 1,
            "scalingConfiguration": { "scalingType": "TARGET_TRACKING_SCALING", "maxCapacity": most },
        }));
    }

    /// `bytes` kept at `s3://<bucket>/<key>`.
    pub fn put_object(&self, bucket: &str, key: &str, bytes: &[u8]) {
        let at = format!("{bucket}/{key}");
        let mut held = self.held.lock().unwrap();
        held.objects.retain(|(a, _)| *a != at);
        held.objects.push((at, bytes.to_vec()));
    }
}

/// A request's operation: `X-Amz-Target`'s, STS's `Action`, or S3's
/// method and path.
pub fn operation(r: &Request) -> String {
    if let Some(target) = r.header("x-amz-target") {
        return target.rsplit('.').next().unwrap_or(target).to_string();
    }
    if let Some(action) = r.body.split('&').find_map(|p| p.strip_prefix("Action=")) {
        return action.to_string();
    }
    format!("{} {}", r.method, r.path)
}

fn amz(status: u16, body: Value) -> Answer {
    (
        status,
        vec![("Content-Type".into(), "application/x-amz-json-1.1".into())],
        body.to_string(),
    )
}

fn not_found(kind: &str, what: &str) -> Answer {
    amz(
        400,
        json!({ "__type": kind, "message": format!("{what} not found") }),
    )
}

fn answer(held: &Mutex<Held>, r: &Request) -> Answer {
    let held = held.lock().unwrap();
    let body: Value = serde_json::from_str(&r.body).unwrap_or(Value::Null);
    let names = |key: &str| -> Vec<String> {
        body[key]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(String::from))
            .collect()
    };
    match operation(r).as_str() {
        "BatchGetBuilds" => {
            let ids = names("ids");
            let builds: Vec<&Value> = held
                .builds
                .iter()
                .filter(|b| {
                    ids.iter()
                        .any(|i| b["id"] == i.as_str() || b["arn"] == i.as_str())
                })
                .collect();
            amz(200, json!({ "builds": builds, "buildsNotFound": [] }))
        }
        "ListBuildsForProject" => {
            let project = body["projectName"].as_str().unwrap_or_default();
            let ids: Vec<&Value> = held
                .builds
                .iter()
                .rev()
                .filter(|b| b["projectName"] == project)
                .map(|b| &b["id"])
                .collect();
            amz(200, json!({ "ids": ids }))
        }
        "BatchGetProjects" => {
            let wanted = names("names");
            let projects: Vec<&Value> = held
                .projects
                .iter()
                .filter(|p| {
                    wanted
                        .iter()
                        .any(|n| p["name"] == n.as_str() || p["arn"] == n.as_str())
                })
                .collect();
            amz(200, json!({ "projects": projects, "projectsNotFound": [] }))
        }
        "BatchGetFleets" => {
            let wanted = names("names");
            let fleets: Vec<&Value> = held
                .fleets
                .iter()
                .filter(|f| {
                    wanted
                        .iter()
                        .any(|n| f["arn"] == n.as_str() || f["name"] == n.as_str())
                })
                .collect();
            amz(200, json!({ "fleets": fleets, "fleetsNotFound": [] }))
        }
        "Sign" => {
            let Some((alias, key)) = &held.key else {
                return not_found("NotFoundException", "the key");
            };
            let asked = body["KeyId"].as_str().unwrap_or_default();
            if asked != KEY && asked != alias {
                return not_found("NotFoundException", asked);
            }
            let digest = STANDARD
                .decode(body["Message"].as_str().unwrap_or_default())
                .unwrap();
            assert_eq!(body["MessageType"], "DIGEST");
            assert_eq!(body["SigningAlgorithm"], "ECDSA_SHA_256");
            let sig: Signature = key.sign_prehash(&digest).unwrap();
            amz(
                200,
                json!({
                    "KeyId": KEY,
                    "Signature": STANDARD.encode(sig.to_der().as_bytes()),
                    "SigningAlgorithm": "ECDSA_SHA_256",
                }),
            )
        }
        "GetPublicKey" => {
            let Some((alias, key)) = &held.key else {
                return not_found("NotFoundException", "the key");
            };
            let asked = body["KeyId"].as_str().unwrap_or_default();
            if asked != KEY && asked != alias {
                return not_found("NotFoundException", asked);
            }
            amz(
                200,
                json!({
                    "KeyId": KEY,
                    "KeySpec": held.spec.as_deref().unwrap_or("ECC_NIST_P256"),
                    "KeyUsage": "SIGN_VERIFY",
                    "PublicKey": public(key),
                    "SigningAlgorithms": ["ECDSA_SHA_256"],
                }),
            )
        }
        "GetCallerIdentity" => (
            200,
            vec![("Content-Type".into(), "text/xml".into())],
            format!(
                "<GetCallerIdentityResponse xmlns=\"https://sts.amazonaws.com/doc/2011-06-15/\">\
                 <GetCallerIdentityResult><Arn>{}</Arn><UserId>AROA:host</UserId>\
                 <Account>{ACCOUNT}</Account></GetCallerIdentityResult>\
                 <ResponseMetadata><RequestId>1</RequestId></ResponseMetadata>\
                 </GetCallerIdentityResponse>",
                held.identity
            ),
        ),
        _ if r.method == "GET" => {
            let path = r
                .path
                .split('?')
                .next()
                .unwrap_or_default()
                .trim_start_matches('/');
            match held.objects.iter().find(|(at, _)| at == path) {
                Some((_, bytes)) => http::bytes(200, "application/zip", bytes),
                None => (
                    404,
                    vec![("Content-Type".into(), "application/xml".into())],
                    "<Error><Code>NoSuchKey</Code><Message>The specified key does not exist.</Message></Error>".into(),
                ),
            }
        }
        other => amz(
            400,
            json!({ "__type": "UnknownOperationException", "message": other }),
        ),
    }
}

/// `key`'s public half as KMS's `GetPublicKey` answers it and a policy
/// lists it: the base64 of its DER SubjectPublicKeyInfo.
pub fn public(key: &SigningKey) -> String {
    const PREFIX: &[u8] = &[
        0x30, 0x59, 0x30, 0x13, 0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06, 0x08,
        0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07, 0x03, 0x42, 0x00,
    ];
    let point = key.verifying_key().to_encoded_point(false);
    STANDARD.encode([PREFIX, point.as_bytes()].concat())
}

/// The files under `dir`, zipped as CodeBuild zips an artifact.
pub fn zip_of(dir: &std::path::Path) -> Vec<u8> {
    use std::io::Write as _;
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let mut entries: Vec<_> = std::fs::read_dir(&d)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .collect();
        entries.sort();
        for p in entries {
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let name = p.strip_prefix(dir).unwrap().to_str().unwrap().to_string();
            zip.start_file(name, options).unwrap();
            zip.write_all(&std::fs::read(&p).unwrap()).unwrap();
        }
    }
    zip.finish().unwrap().into_inner()
}

/// The hex SHA-256 of `bytes`.
pub fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
