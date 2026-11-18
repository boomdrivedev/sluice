//! The CloudFormation template `sluice init codebuild` writes (record 0119;
//! design D10 of the change `a-codebuild-fleet-runs-a-queues-checks`):
//! for each Linux system it is given a reserved-capacity fleet and a run
//! project on it, holding no key; the sign project its run builds' ends
//! start through the rule; the hand-over bucket, the signing key, the
//! secret for the app's key and the roles. It is rendered with the release
//! that writes it, which its builds install, and the repository they read.
//! The template's one system's resources stand between `# system` and
//! `# end system`, written again for each system given.

use anyhow::Result;

use crate::own_steps::{self, Carry, Layout, Reading, Seg};

/// Where the template lives in a repository.
pub const PATH: &str = ".config/sluice-codebuild.yml";

/// The lines that open and close a run buildspec's room for the
/// repository's own commands, among its `post_build` phase's commands
/// (record 0119 as amended 2026-11-18; design D1 of the change
/// `a-pools-run-job-runs-the-repositorys-own-steps`).
pub const ROOM_OPEN: &str = "# sluice: your own commands, after the checks, in $SLUICE_CHECKOUT";
pub const ROOM_CLOSE: &str = "# sluice: end of your own commands";

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
        logical(&self.name)
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

/// A system's name as a CloudFormation logical id's part: `linux-x64` is
/// `LinuxX64`.
fn logical(name: &str) -> String {
    name.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_ascii_uppercase().to_string() + c.as_str())
                .unwrap_or_default()
        })
        .collect()
}

/// The system a template's line at `at` is of: the nearest `# system`
/// line above it.
fn system_of(lines: &[&str], at: usize) -> String {
    lines[..at]
        .iter()
        .rev()
        .find_map(|l| {
            l.trim()
                .strip_prefix("# system ")
                .map(|n| n.trim().to_string())
        })
        .unwrap_or_default()
}

/// The commands of `system`'s run project's `post_build` phase, which its
/// room's entries join: in its buildspec, read as YAML of its own.
fn post_build_of(system: &str) -> Vec<Seg> {
    let mut path = own_steps::keys(&[
        "Resources",
        &format!("Run{}", logical(system)),
        "Properties",
        "Source",
        "BuildSpec",
    ]);
    path.push(Seg::Text);
    path.extend(own_steps::keys(&["phases", "post_build", "commands"]));
    path
}

/// The commands of a buildspec's `post_build` phase.
fn post_build(_: &str) -> Vec<Seg> {
    own_steps::keys(&["phases", "post_build", "commands"])
}

/// What a template's rooms are: one in each run project's buildspec, by
/// its system.
fn layout() -> Layout<'static> {
    Layout {
        open: ROOM_OPEN,
        close: ROOM_CLOSE,
        key: &system_of,
        list: &post_build_of,
        own_line: None,
    }
}

/// `had`, a template, read against `wants`, what this sluice writes it as
/// (design D3): as sluice writes it, with or without the repository's own
/// commands; differing; or a run project's own commands leaving its
/// buildspec's `post_build` commands, by its system.
pub fn read(had: &str, wants: &str) -> Result<Reading> {
    own_steps::read(had, wants, &layout())
}

/// `wants` with each run project's own commands in `had` carried into the
/// room of the run project of its system; a system `wants` writes none for
/// said in `dropped` (design D3).
pub fn carry(had: &str, wants: &str) -> Result<Carry> {
    own_steps::carry(had, wants, &layout())
}

/// `had`, the buildspec of `system`'s run project as CodeBuild holds it,
/// read against `wants`, as this sluice writes it (design D3).
pub fn read_buildspec(had: &str, wants: &str, system: &str) -> Result<Reading> {
    let key = |_: &[&str], _: usize| system.to_string();
    own_steps::read(
        had,
        wants,
        &Layout {
            open: ROOM_OPEN,
            close: ROOM_CLOSE,
            key: &key,
            list: &post_build,
            own_line: None,
        },
    )
}

/// What a template or a buildspec as sluice writes it holds of the
/// repository's own, as said after "as sluice writes it" or "written".
pub fn holds(reading: &Reading) -> &'static str {
    match reading {
        Reading::AsWritten { rooms, .. } if !rooms.is_empty() => {
            ", with the repository's own commands"
        }
        _ => "",
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

    /// Record 0119 (design D3; C.8): the lead role may start the run
    /// projects alone; a start that carries any override CodeBuild keys,
    /// or a variable but the five, is denied; and it holds no use of the
    /// key, no secret and of the sign project only the reading of its
    /// builds.
    #[test]
    fn the_lead_may_change_nothing_a_run_build_runs() {
        let stack = stack();
        let lead = resource(&stack, "LeadRole");
        let all = statements(lead);
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
        let text = said(lead);
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
        // Record 0119 as amended 2026-11-18 (design D5): the run role
        // allows none of them; its denies are read in
        // `the_run_role_takes_the_repositorys_policies_and_its_denies_stand`.
        let allowed: Vec<Yaml> = statements(resource(&stack, "RunRole"))
            .into_iter()
            .filter(|s| s["Effect"].as_str() == Some("Allow"))
            .collect();
        for forbidden in ["kms:", "secretsmanager:", "codebuild:", "iam:"] {
            for s in &allowed {
                assert!(
                    !said(s).contains(forbidden),
                    "the run role allows {forbidden}: {}",
                    said(s)
                );
            }
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
        let signers: Vec<&str> = ["RunRole", "SignRole", "EventsRole", "LeadRole"]
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

    /// Record 0119 (design D2): the rule matches a run project's build that
    /// ended `SUCCEEDED`, `FAILED`, `FAULT` or `TIMED_OUT`, never one
    /// stopped, and starts the sign project with `SLUICE_RUN_BUILD` from
    /// the event's build, and nothing the lead chose.
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

    /// Record 0119 as amended 2026-11-18 (design D1 and D2 of the change
    /// `a-pools-run-job-runs-the-repositorys-own-steps`; timber 135; rule
    /// core.63): each run buildspec runs `sluice job run` naming its
    /// checkout as its build phase's last command; its `post_build` phase
    /// exports sluice's `PATH` and `SLUICE_CHECKOUT`, then leaves its room,
    /// the two `# sluice:` lines with nothing between at the commands'
    /// indentation; and its `finally` block releases the checkout in the
    /// clone, whatever the room's commands came to.
    #[test]
    fn each_run_build_leaves_room_for_the_repositorys_own_commands() {
        let stack = stack();
        for system in ["RunLinuxX64", "RunLinuxArm64"] {
            let (spec, text) = buildspec(&stack, system);
            let last = commands(&spec).last().cloned().unwrap_or_default();
            assert_eq!(
                last.lines().map(str::trim).rfind(|l| !l.is_empty()),
                Some(
                    "sluice job run --claim \"$SLUICE_CLAIM\" --branch \"$SLUICE_BRANCH\" \
                     --out \"$CODEBUILD_SRC_DIR/sluice-run\" --checkout \
                     \"$CODEBUILD_SRC_DIR/sluice-checkout\""
                ),
                "{system}: {text}"
            );
            let post = &spec["phases"]["post_build"];
            let first = post["commands"][0].as_str().unwrap_or_default();
            assert!(
                first.contains("export PATH=\"$HOME/.local/bin:$PATH\"")
                    && first.contains(
                        "export SLUICE_CHECKOUT=\"$(head -n 1 \"$CODEBUILD_SRC_DIR/sluice-checkout\")\""
                    ),
                "{system}: {first}"
            );
            assert_eq!(
                post["commands"].as_vec().map(Vec::len),
                Some(1),
                "{system}: nothing of sluice's in the room"
            );
            let lines: Vec<&str> = text.lines().collect();
            let open = lines
                .iter()
                .position(|l| {
                    *l == "      # sluice: your own commands, after the checks, in $SLUICE_CHECKOUT"
                })
                .unwrap_or_else(|| panic!("{system} has no room: {text}"));
            assert_eq!(lines[open + 1], "      # sluice: end of your own commands");
            let exported = lines
                .iter()
                .position(|l| l.contains("export SLUICE_CHECKOUT="))
                .unwrap();
            let finally = lines
                .iter()
                .position(|l| *l == "    finally:")
                .unwrap_or_else(|| panic!("{system} has no finally: {text}"));
            assert!(exported < open && open + 1 < finally, "{system}: {text}");
            let released = post["finally"][0].as_str().unwrap_or_default();
            assert!(
                released.contains("cd \"$CODEBUILD_SRC_DIR/repository\"")
                    && released.contains(
                        "sluice job release --checkout \"$CODEBUILD_SRC_DIR/sluice-checkout\" \
                         --out \"$CODEBUILD_SRC_DIR/sluice-run\""
                    ),
                "{system}: {released}"
            );
        }
    }

    /// The value `y` of a template takes where its parameters are
    /// `params`: `Ref`, `Fn::Join`, `Fn::Equals`, `Fn::Not` and `Fn::If`
    /// as CloudFormation reads them, enough for the run role's policies.
    fn resolved(stack: &Yaml, y: &Yaml, params: &[(&str, Vec<&str>)]) -> Option<Yaml> {
        let h = y.as_hash()?;
        let one = |k: &str| h.get(&Yaml::String(k.into()));
        if let Some(name) = one("Ref").and_then(Yaml::as_str) {
            if name == "AWS::NoValue" {
                return None;
            }
            let (_, v) = params.iter().find(|(p, _)| *p == name)?;
            return Some(Yaml::Array(
                v.iter().map(|s| Yaml::String((*s).into())).collect(),
            ));
        }
        let strings = |y: Option<Yaml>| match y {
            Some(Yaml::Array(a)) => a
                .iter()
                .filter_map(|s| s.as_str().map(String::from))
                .collect::<Vec<_>>(),
            Some(Yaml::String(s)) => vec![s],
            _ => Vec::new(),
        };
        if let Some(args) = one("Fn::Join").and_then(Yaml::as_vec) {
            let joined = strings(resolved(stack, &args[1], params)).join(args[0].as_str()?);
            return Some(Yaml::String(joined));
        }
        if let Some(args) = one("Fn::Equals").and_then(Yaml::as_vec) {
            let side = |y: &Yaml| match y {
                Yaml::String(s) => Some(s.clone()),
                _ => resolved(stack, y, params)?.as_str().map(String::from),
            };
            return Some(Yaml::Boolean(side(&args[0]) == side(&args[1])));
        }
        if let Some(args) = one("Fn::Not").and_then(Yaml::as_vec) {
            return Some(Yaml::Boolean(
                !resolved(stack, &args[0], params)?.as_bool()?,
            ));
        }
        if let Some(args) = one("Fn::If").and_then(Yaml::as_vec) {
            let condition = &stack["Conditions"][args[0].as_str()?];
            let chosen = if resolved(stack, condition, params)?.as_bool()? {
                &args[1]
            } else {
                &args[2]
            };
            return resolved(stack, chosen, params);
        }
        Some(y.clone())
    }

    /// Record 0119 as amended 2026-11-18 (design D5; timber 135; rule
    /// core.62): the run role is given the managed policies `RunPolicies`
    /// names, none where it names none, and its own policy denies every
    /// `kms:` action on the signing key, every `secretsmanager:` action on
    /// the app's key and every `codebuild:` and `iam:` action, whichever.
    #[test]
    fn the_run_role_takes_the_repositorys_policies_and_its_denies_stand() {
        let stack = stack();
        let parameter = &stack["Parameters"]["RunPolicies"];
        assert_eq!(parameter["Type"].as_str(), Some("CommaDelimitedList"));
        assert_eq!(parameter["Default"].as_str(), Some(""));
        let role = resource(&stack, "RunRole");
        let managed = &role["Properties"]["ManagedPolicyArns"];
        let store = "arn:aws:iam::111122223333:policy/write-the-cache";
        assert_eq!(
            resolved(&stack, managed, &[("RunPolicies", vec![store])]),
            Some(Yaml::Array(vec![Yaml::String(store.into())])),
            "one policy named, given"
        );
        assert_eq!(
            resolved(&stack, managed, &[("RunPolicies", vec![""])]),
            None,
            "none named, none given"
        );
        let denied: Vec<(String, Yaml)> = statements(role)
            .iter()
            .filter(|s| s["Effect"].as_str() == Some("Deny"))
            .flat_map(|s| {
                let on = s["Resource"].clone();
                actions(s).into_iter().map(move |a| (a, on.clone()))
            })
            .collect();
        for (action, on) in [
            ("kms:*", "Fn::GetAtt: [SignKey, Arn]"),
            ("secretsmanager:*", "Ref: AppKey"),
            ("codebuild:*", "\"*\""),
            ("iam:*", "\"*\""),
        ] {
            let on = YamlLoader::load_from_str(on).unwrap().remove(0);
            assert!(
                denied.iter().any(|(a, r)| a == action && *r == on),
                "the run role denies {action} on {on:?}: {denied:?}"
            );
        }
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
            // Record 0119 as amended 2026-11-18 (design D2): the file
            // naming a checkout an earlier build kept goes too.
            assert!(
                lines[at("rm -rf \"/tmp/sluice-$(id -u)\"")]
                    .contains("\"$CODEBUILD_SRC_DIR/sluice-checkout\""),
                "{text}"
            );
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
