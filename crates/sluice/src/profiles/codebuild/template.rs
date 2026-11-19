//! The CloudFormation template `sluice init codebuild` writes (record 0119;
//! design D10 of the change `a-codebuild-fleet-runs-a-queues-checks`):
//! for each Linux system it is given a reserved-capacity fleet and a run
//! project on it, holding no key; the sign project its run builds' ends
//! start through the rule; the hand-over bucket, the signing key, the
//! secret for the app's key and the roles. It is rendered with the release
//! that writes it, which its builds install, and the repository they read.
//! The template's one system's resources stand between `# system` and
//! `# end system`, written again for each system given.

/// Where the template lives in a repository.
pub const PATH: &str = ".config/sluice-codebuild.yml";

const TEMPLATE: &str = include_str!("templates/stack.yml");

/// The lines that open and close the template's resources of one system.
const BEGIN: &str = "  # system linux-x64\n";
const END: &str = "  # end system\n";

/// A system CodeBuild runs on Linux, as the policy names it, and its
/// processor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct System {
    pub name: String,
    pub arch: Arch,
}

/// The processors CodeBuild's Linux fleets run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arch {
    X86_64,
    Aarch64,
}

impl System {
    /// The policy's system `name`, of `os` and `arch`, where CodeBuild runs
    /// it on Linux; `None` otherwise.
    pub fn linux(name: &str, os: &str, arch: &str) -> Option<System> {
        let arch = match (os, arch) {
            ("linux", "x86_64") => Arch::X86_64,
            ("linux", "aarch64") => Arch::Aarch64,
            _ => return None,
        };
        Some(System {
            name: name.into(),
            arch,
        })
    }

    /// Its name as a CloudFormation logical id's part: `linux-x64` is
    /// `LinuxX64`.
    fn logical(&self) -> String {
        self.name
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(|w| {
                let mut c = w.chars();
                c.next()
                    .map(|f| f.to_ascii_uppercase().to_string() + c.as_str())
                    .unwrap_or_default()
            })
            .collect()
    }

    /// The template's one system's resources, written for this one.
    fn resources(&self, block: &str) -> String {
        let (environment, release, image) = match self.arch {
            Arch::X86_64 => ("LINUX_EC2", "x86_64-linux", "amazonlinux-x86_64-base"),
            Arch::Aarch64 => ("ARM_EC2", "aarch64-linux", "amazonlinux-arm-base"),
        };
        block
            .replace("# system linux-x64", &format!("# system {}", self.name))
            .replace("LinuxX64", &self.logical())
            .replace("linux-x64", &self.name)
            .replace("LINUX_EC2", environment)
            .replace("x86_64-linux", release)
            .replace("amazonlinux-x86_64-base", image)
    }
}

/// The template as sluice `version` writes it for `repository`,
/// `owner/name` on GitHub, with a fleet and a run project for each of
/// `systems`.
pub fn render(version: &str, repository: &str, systems: &[System]) -> String {
    let start = TEMPLATE.find(BEGIN).expect("the template's system");
    let end = TEMPLATE[start..].find(END).expect("its end") + start + END.len();
    let block = &TEMPLATE[start..end];
    let each: String = systems.iter().map(|s| s.resources(block)).collect();
    format!("{}{each}{}", &TEMPLATE[..start], &TEMPLATE[end..])
        .replace("{{version}}", version)
        .replace("{{repository}}", repository)
}

/// The name a stack's run project of `system` has, as the template names
/// it.
pub fn run_project(stack: &str, system: &str) -> String {
    format!("{stack}-run-{system}")
}

/// The name a stack's sign project has.
pub fn sign_project(stack: &str) -> String {
    format!("{stack}-sign")
}

/// The alias a stack's key has.
pub fn key_alias(stack: &str) -> String {
    format!("alias/{stack}-sign")
}

/// The buildspec the template gives the run projects of `system`, as sluice
/// `version` writes it for `repository`, and the sign project's: what
/// `sluice pool check` reads a project's back against (design D11).
pub fn buildspecs(version: &str, repository: &str, system: &System) -> (String, String) {
    let rendered = render(version, repository, std::slice::from_ref(system));
    let specs: Vec<String> = rendered
        .split("        BuildSpec: |\n")
        .skip(1)
        .map(|rest| {
            rest.lines()
                .take_while(|l| l.starts_with("          ") || l.is_empty())
                .map(|l| l.strip_prefix("          ").unwrap_or(l))
                .collect::<Vec<_>>()
                .join("\n")
                .trim_end()
                .to_string()
                + "\n"
        })
        .collect();
    (specs[0].clone(), specs[1].clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use yaml_rust2::{Yaml, YamlLoader};

    fn systems() -> Vec<System> {
        vec![
            System::linux("linux-x64", "linux", "x86_64").unwrap(),
            System::linux("linux-arm64", "linux", "aarch64").unwrap(),
        ]
    }

    /// The template rendered for both systems, read as YAML.
    fn stack() -> Yaml {
        let text = render("1.2.3", "acme/widgets", &systems());
        YamlLoader::load_from_str(&text).unwrap().remove(0)
    }

    fn resource<'a>(stack: &'a Yaml, name: &str) -> &'a Yaml {
        let r = &stack["Resources"][name];
        assert!(!r.is_badvalue(), "no resource {name}");
        r
    }

    /// Every statement of a role's inline policies.
    fn statements(role: &Yaml) -> Vec<Yaml> {
        role["Properties"]["Policies"]
            .as_vec()
            .into_iter()
            .flatten()
            .flat_map(|p| {
                p["PolicyDocument"]["Statement"]
                    .as_vec()
                    .cloned()
                    .unwrap_or_default()
            })
            .collect()
    }

    /// A statement's actions, one or a list.
    fn actions(statement: &Yaml) -> Vec<String> {
        match &statement["Action"] {
            Yaml::String(a) => vec![a.clone()],
            Yaml::Array(a) => a
                .iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Everything a YAML value says, as one string: its keys and values.
    fn said(y: &Yaml) -> String {
        let mut out = String::new();
        let mut emitter = yaml_rust2::YamlEmitter::new(&mut out);
        emitter.dump(y).unwrap();
        out
    }

    /// A project's inline buildspec, read as YAML, and its text.
    fn buildspec(stack: &Yaml, project: &str) -> (Yaml, String) {
        let text = resource(stack, project)["Properties"]["Source"]["BuildSpec"]
            .as_str()
            .unwrap()
            .to_string();
        (YamlLoader::load_from_str(&text).unwrap().remove(0), text)
    }

    /// Each command of a buildspec's build phase.
    fn commands(spec: &Yaml) -> Vec<String> {
        spec["phases"]["build"]["commands"]
            .as_vec()
            .into_iter()
            .flatten()
            .filter_map(|c| c.as_str().map(String::from))
            .collect()
    }

    /// Record 0119 (design D3; C.8): the start role may start the run
    /// projects alone; a start that carries any override CodeBuild keys,
    /// or a variable but the five, is denied; and it holds no use of the
    /// key, no secret and of the sign project only the reading of its
    /// builds.
    #[test]
    fn the_start_role_may_change_nothing_a_run_build_runs() {
        let stack = stack();
        let start = resource(&stack, "StartRole");
        let all = statements(start);
        let allowed: Vec<&Yaml> = all
            .iter()
            .filter(|s| s["Effect"].as_str() == Some("Allow"))
            .collect();
        let starts: Vec<&&Yaml> = allowed
            .iter()
            .filter(|s| actions(s).iter().any(|a| a == "codebuild:StartBuild"))
            .collect();
        assert_eq!(starts.len(), 1);
        let on = said(&starts[0]["Resource"]);
        assert!(on.contains("project/${AWS::StackName}-run-*"), "{on}");
        assert!(!on.contains("Sign"), "{on}");
        let denied: Vec<&Yaml> = all
            .iter()
            .filter(|s| s["Effect"].as_str() == Some("Deny"))
            .collect();
        assert!(
            denied
                .iter()
                .all(|s| actions(s) == ["codebuild:StartBuild"]
                    && s["Resource"].as_str() == Some("*"))
        );
        let null_keys: Vec<String> = denied
            .iter()
            .filter_map(|s| s["Condition"]["Null"].as_hash())
            .flat_map(|h| {
                assert_eq!(h.len(), 1, "one key a statement, so any one denies");
                h.iter().map(|(k, v)| {
                    assert_eq!(v.as_str(), Some("false"));
                    k.as_str().unwrap().to_string()
                })
            })
            .collect();
        for key in [
            "codebuild:source.buildspec",
            "codebuild:environment.image",
            "codebuild:serviceRole",
            "codebuild:environment.privilegedMode",
            "codebuild:environment.fleet.fleetArn",
            "codebuild:environment.computeType",
            "codebuild:environment.type",
            "codebuild:environment.imagePullCredentialsType",
            "codebuild:source.location",
            "codebuild:artifacts.location",
            "codebuild:cache.type",
            "codebuild:logsConfig.cloudWatchLogs.status",
            "codebuild:secondarySources.location",
        ] {
            assert!(null_keys.iter().any(|k| k == key), "{key} not denied");
        }
        let names: Vec<String> = denied
            .iter()
            .filter_map(|s| {
                s["Condition"]["ForAnyValue:StringNotEquals"]
                    ["codebuild:environment.environmentVariables.name"]
                    .as_vec()
            })
            .flatten()
            .filter_map(|n| n.as_str().map(String::from))
            .collect();
        assert_eq!(
            names,
            [
                "SLUICE_CLAIM",
                "SLUICE_BRANCH",
                "SLUICE_COMBINATION",
                "SLUICE_SYSTEM",
                "SLUICE_READ_TOKEN"
            ]
        );
        let text = said(start);
        assert!(!text.contains("kms:"), "no use of the key");
        assert!(!text.contains("secretsmanager:"), "no secret");
        for s in &all {
            if said(&s["Resource"]).contains("[Sign, Arn]")
                || said(&s["Resource"]).contains("- Sign\n")
            {
                assert_eq!(
                    actions(s),
                    ["codebuild:BatchGetBuilds", "codebuild:ListBuildsForProject"],
                    "the sign project only read"
                );
            }
        }
    }

    /// Record 0119 (design D2; rule core.62): each run project's role names
    /// no key, no secret and no build, and its buildspec runs `sluice job
    /// run` and nothing that signs; only the sign role may sign with the
    /// key, as `ECDSA_SHA_256` over a digest, and read the app's key, and
    /// the sign buildspec runs sluice's `job sign` and `job record` alone.
    #[test]
    fn the_run_build_holds_no_key_and_the_sign_build_runs_no_check() {
        let stack = stack();
        let run = said(resource(&stack, "RunRole"));
        for forbidden in ["kms:", "secretsmanager:", "codebuild:"] {
            assert!(!run.contains(forbidden), "the run role names {forbidden}");
        }
        for system in ["RunLinuxX64", "RunLinuxArm64"] {
            let project = resource(&stack, system);
            assert!(said(&project["Properties"]["ServiceRole"]).contains("RunRole"));
            assert!(project["Properties"]["Environment"]["EnvironmentVariables"].is_badvalue());
            let (_, text) = buildspec(&stack, system);
            assert!(text.contains("sluice job run --claim"), "{text}");
            for signs in ["job sign", "job record", "kms", "SLUICE_APP_KEY"] {
                assert!(!text.contains(signs), "{system} names {signs}");
            }
        }
        // The key's policy lets the sign role alone sign.
        let key = resource(&stack, "SignKey");
        assert_eq!(key["Properties"]["KeySpec"].as_str(), Some("ECC_NIST_P256"));
        assert_eq!(key["Properties"]["KeyUsage"].as_str(), Some("SIGN_VERIFY"));
        let key_policy = key["Properties"]["KeyPolicy"]["Statement"]
            .as_vec()
            .unwrap();
        for s in key_policy {
            let a = actions(s);
            let signs = a
                .iter()
                .any(|x| x == "kms:Sign" || x == "kms:*" || x == "kms:Sign*");
            if signs {
                assert!(said(&s["Principal"]).contains("SignRole"), "{}", said(s));
                assert_eq!(
                    s["Condition"]["StringEquals"]["kms:MessageType"].as_str(),
                    Some("DIGEST")
                );
            }
        }
        // Of the roles, the sign role's policy alone signs and reads the
        // secret.
        let signers: Vec<&str> = ["RunRole", "SignRole", "EventsRole", "StartRole"]
            .into_iter()
            .filter(|r| said(resource(&stack, r)).contains("kms:Sign"))
            .collect();
        assert!(signers.is_empty(), "{signers:?}");
        let with = resource(&stack, "SignWithTheKey");
        assert!(said(&with["Properties"]["RoleName"]).contains("SignRole"));
        let s = &with["Properties"]["PolicyDocument"]["Statement"][0];
        assert_eq!(actions(s), ["kms:Sign"]);
        assert_eq!(
            s["Condition"]["StringEquals"]["kms:SigningAlgorithm"].as_str(),
            Some("ECDSA_SHA_256")
        );
        assert_eq!(
            s["Condition"]["StringEquals"]["kms:MessageType"].as_str(),
            Some("DIGEST")
        );
        assert!(said(resource(&stack, "SignRole")).contains("secretsmanager:GetSecretValue"));
        let (spec, _) = buildspec(&stack, "Sign");
        let sluices: Vec<String> = commands(&spec)
            .iter()
            .flat_map(|c| {
                c.lines()
                    .map(str::trim)
                    .map(String::from)
                    .collect::<Vec<_>>()
            })
            .filter(|l| l.starts_with("sluice "))
            .collect();
        assert_eq!(
            sluices,
            [
                "sluice job sign --build \"$SLUICE_RUN_BUILD\" --out \"$CODEBUILD_SRC_DIR/sign\"",
                "sluice job record --build \"$SLUICE_RUN_BUILD\" --from \"$CODEBUILD_SRC_DIR/sign\"",
            ]
        );
        assert!(!said(&spec).contains("moon") && !said(&spec).contains("job run"));
    }

    /// Record 0058 as amended 2026-11-18 (core.26): the parameter is
    /// `StartPrincipal`, the role and its output `StartRole`, the role's
    /// policy `start`, and no name, comment or description says `lead`, in
    /// any case.
    #[test]
    fn the_template_names_the_start_role_and_no_lead() {
        let text = render("1.2.3", "acme/widgets", &systems());
        let stack = YamlLoader::load_from_str(&text).unwrap().remove(0);
        assert!(!stack["Parameters"]["StartPrincipal"].is_badvalue());
        let start = resource(&stack, "StartRole");
        assert!(said(&start["Properties"]["AssumeRolePolicyDocument"]).contains("StartPrincipal"));
        let policies: Vec<&str> = start["Properties"]["Policies"]
            .as_vec()
            .into_iter()
            .flatten()
            .filter_map(|p| p["PolicyName"].as_str())
            .collect();
        assert_eq!(policies, ["start"]);
        assert_eq!(
            stack["Outputs"]["StartRole"]["Value"]["Fn::GetAtt"][0].as_str(),
            Some("StartRole")
        );
        assert!(!text.to_lowercase().contains("lead"), "{text}");
    }

    /// Record 0119 (design D2): the rule matches a run project's build that
    /// ended `SUCCEEDED`, `FAILED`, `FAULT` or `TIMED_OUT`, never one
    /// stopped, and starts the sign project with `SLUICE_RUN_BUILD` from
    /// the event's build, and nothing the host that started the run build
    /// chose.
    #[test]
    fn the_rule_starts_the_sign_build_for_every_end_but_a_stop() {
        let stack = stack();
        let rule = &resource(&stack, "RunEnded")["Properties"];
        let pattern = &rule["EventPattern"];
        assert_eq!(pattern["source"][0].as_str(), Some("aws.codebuild"));
        let ends: Vec<&str> = pattern["detail"]["build-status"]
            .as_vec()
            .unwrap()
            .iter()
            .filter_map(Yaml::as_str)
            .collect();
        assert_eq!(ends, ["SUCCEEDED", "FAILED", "FAULT", "TIMED_OUT"]);
        assert!(said(&pattern["detail"]["project-name"]).contains("${AWS::StackName}-run-"));
        let targets = rule["Targets"].as_vec().unwrap();
        assert_eq!(targets.len(), 1);
        let target = &targets[0];
        assert!(said(&target["Arn"]).contains("Sign"));
        assert_eq!(
            target["InputTransformer"]["InputPathsMap"]["build"].as_str(),
            Some("$.detail.build-id")
        );
        let input = target["InputTransformer"]["InputTemplate"]
            .as_str()
            .unwrap();
        let filled: serde_json::Value =
            serde_json::from_str(&input.replace("<build>", "\"arn:build\"")).unwrap();
        assert_eq!(
            filled,
            serde_json::json!({ "environmentVariablesOverride": [
                { "name": "SLUICE_RUN_BUILD", "value": "arn:build", "type": "PLAINTEXT" }
            ] })
        );
        // Only the rule's role may start the sign project.
        let events = said(resource(&stack, "EventsRole"));
        assert!(events.contains("codebuild:StartBuild") && events.contains("Sign"));
    }

    /// Record 0119 (design D8; rule core.10): each run buildspec, before
    /// `sluice job run`, ends every container the instance's engine runs and
    /// removes the build user's sluice scratch directory and the state an
    /// earlier build kept, its images left; and the template has a fleet
    /// and a run project for each system, of its own environment.
    #[test]
    fn a_run_build_starts_with_nothing_of_an_earlier_build_running() {
        let stack = stack();
        for (system, fleet, environment, release) in [
            (
                "RunLinuxX64",
                "FleetLinuxX64",
                "LINUX_EC2",
                "sluice-x86_64-linux",
            ),
            (
                "RunLinuxArm64",
                "FleetLinuxArm64",
                "ARM_EC2",
                "sluice-aarch64-linux",
            ),
        ] {
            let (spec, text) = buildspec(&stack, system);
            let lines: Vec<String> = commands(&spec)
                .iter()
                .flat_map(|c| {
                    c.lines()
                        .map(str::trim)
                        .map(String::from)
                        .collect::<Vec<_>>()
                })
                .collect();
            let at = |what: &str| {
                lines
                    .iter()
                    .position(|l| l.contains(what))
                    .unwrap_or_else(|| panic!("{system} has no {what}: {text}"))
            };
            let run = at("sluice job run");
            assert!(at("docker rm --force") < run);
            assert!(at("docker ps --all --quiet") < run);
            assert!(at("rm -rf \"/tmp/sluice-$(id -u)\"") < run);
            assert!(lines[at("rm -rf \"/tmp/sluice-$(id -u)\"")].contains("/sluice\""));
            assert!(
                !text.contains("rmi") && !text.contains("image prune"),
                "images stay"
            );
            assert!(text.contains(release), "{text}");
            let project = &resource(&stack, system)["Properties"];
            assert_eq!(project["Environment"]["Type"].as_str(), Some(environment));
            assert!(said(&project["Environment"]["Fleet"]).contains(fleet));
            assert_eq!(project["Source"]["Type"].as_str(), Some("NO_SOURCE"));
            assert_eq!(project["Artifacts"]["Packaging"].as_str(), Some("ZIP"));
            assert!(said(&project["TimeoutInMinutes"]).contains("RunTimeout"));
            let f = &resource(&stack, fleet)["Properties"];
            assert_eq!(f["EnvironmentType"].as_str(), Some(environment));
            assert_eq!(f["OverflowBehavior"].as_str(), Some("QUEUE"));
        }
        let text = render("1.2.3", "acme/widgets", &systems());
        assert!(!text.contains("{{"), "every placeholder filled");
        assert!(text.contains("SLUICE_VERSION: \"1.2.3\""));
        assert!(text.contains("SLUICE_REPOSITORY: \"acme/widgets\""));
        let (run, sign) = buildspecs("1.2.3", "acme/widgets", &systems()[1]);
        assert!(run.contains("sluice-aarch64-linux") && run.starts_with("version: 0.2\n"));
        assert!(sign.contains("sluice job sign --build"));
    }
}
