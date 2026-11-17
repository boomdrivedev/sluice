//! Previews on a scratch repository (previews-1 clauses 98 to 101): the
//! binary, a real checkout and a real supervisor. Needs git, so it is in
//! the check-tools tier.

#![cfg(feature = "tools")]

#[path = "support/mod.rs"]
mod support;

use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use support::{Repo, Scratch};

struct Setup {
    repo: Repo,
    log: PathBuf,
    /// Where a preview command that is asked to writes its `SLUICE_PLACE`.
    place: PathBuf,
    config: Scratch,
    state: Scratch,
    /// The fake container engine every `sluice` here is pointed at, so no
    /// test lists or removes a container of the host's (record 0121).
    engine: support::Engine,
    /// What every `sluice` here is given as its engine's sockets.
    sockets: String,
    _engine_dir: Scratch,
}

impl Setup {
    /// `bolt/x` with a preview whose command exits as `command` says and
    /// whose release command logs the name it releases.
    fn new(name: &str, ports_from: u16, command: &str) -> Setup {
        Setup::with(
            name,
            ports_from,
            command,
            "echo released $SLUICE_NAME_WEB >> {log}",
            &[],
        )
    }

    /// `bolt/x` with a preview whose command is `command`, whose release
    /// command is `release`, and with `files` committed beside its policy:
    /// in each, `{log}` stands for the log's path and `{place}` for the
    /// file the test reads the preview's `SLUICE_PLACE` from.
    fn with(
        name: &str,
        ports_from: u16,
        command: &str,
        release: &str,
        files: &[(&str, &str)],
    ) -> Setup {
        let repo = Repo::new(name);
        let log = repo.dir().with_extension("released.log");
        let place = repo.dir().with_extension("place");
        for f in [&log, &place] {
            let _ = std::fs::remove_file(f);
        }
        let fill = |text: &str| {
            text.replace("{log}", &log.display().to_string())
                .replace("{place}", &place.display().to_string())
        };
        repo.write(
            ".config/sluice.toml",
            &format!(
                "[tests]\nrunner = \"command\"\ncommand = \"true\"\n\
                 release = \"{}\"\n\n\
                 [checkout]\nports = [\"web\"]\nports_from = \"{ports_from}\"\n\n\
                 [branches.\"bolt/*\"]\ntimeout = \"1m\"\n\n\
                 [preview]\ncommand = \"{}\"\n",
                fill(release),
                fill(command)
            ),
        );
        for (path, text) in files {
            repo.write(path, &fill(text));
        }
        repo.git(&["add", "-A"]);
        repo.git(&["commit", "-q", "-m", "a preview"]);
        repo.git(&["branch", "bolt/x", "main"]);
        let engine_dir = Scratch::short();
        let engine = support::containers_engine(engine_dir.path());
        Setup {
            repo,
            log,
            place,
            config: Scratch::new(&format!("{name}-config")),
            state: Scratch::new(&format!("{name}-state")),
            sockets: engine.socket().display().to_string(),
            engine,
            _engine_dir: engine_dir,
        }
    }

    fn sluice(&self, args: &[&str]) -> Output {
        support::isolated(&mut Command::new(env!("CARGO_BIN_EXE_sluice")))
            .args(args)
            .current_dir(self.repo.dir())
            .env("XDG_CONFIG_HOME", self.config.path())
            .env("XDG_STATE_HOME", self.state.path())
            .env(sluice::profiles::checks::ENGINE_SOCKETS_VAR, &self.sockets)
            .env_remove("SLUICE_AGENT")
            .env_remove("SLUICE_OTEL")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn preview(&self) -> serde_json::Value {
        let rev = self.repo.rev("bolt/x");
        let out = self.sluice(&["preview", "bolt/x", &rev, "--wait", "--json"]);
        serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("{}", text(&out)))
    }

    /// The ports the host holds for live places, each `held/<port>` (31,
    /// 32; record 0016 as amended 2026-11-13).
    fn held(&self) -> Vec<String> {
        match std::fs::read_dir(self.state.path().join("sluice/ports/held")) {
            Ok(d) => d
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .filter(|n| !n.starts_with('.'))
                .collect(),
            Err(_) => Vec::new(),
        }
    }
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// 101: a preview's release command is the one its revision's policy
/// named, saved when its command started, so it runs after its branch is
/// deleted; the checkout is removed and the port freed.
#[test]
fn a_preview_is_released_after_its_branch_is_gone() {
    let s = Setup::new("preview-branch-gone", 42000, "true");
    let doc = s.preview();
    assert_eq!(doc["state"], "up", "{doc}");
    assert_eq!(s.held().len(), 1);
    s.repo.git(&["branch", "-D", "bolt/x"]);
    let out = s.sluice(&["release", doc["name"].as_str().unwrap()]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(
        std::fs::read_to_string(&s.log).unwrap(),
        format!(
            "released {}\n",
            doc["names"]["SLUICE_NAME_WEB"].as_str().unwrap()
        )
    );
    assert!(!PathBuf::from(doc["checkout"].as_str().unwrap()).exists());
    assert!(s.held().is_empty());
    let out = s.sluice(&["previews"]);
    assert_eq!(text(&out), "no previews\n");
}

/// 101, 123: a preview whose checkout is removed by hand has the release
/// its run stored run once by the next `sluice previews`, with its names
/// in its environment, and is freed, its port and names let go; no later
/// listing runs its release again.
#[test]
fn a_removed_checkout_is_freed_by_the_next_listing() {
    let s = Setup::new("preview-checkout-gone", 42010, "true");
    let doc = s.preview();
    let checkout = doc["checkout"].as_str().unwrap();
    s.repo.git(&["worktree", "remove", "--force", checkout]);
    let out = s.sluice(&["previews"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(
        text(&out),
        format!(
            "freed {}: its checkout no longer exists; its release ran once from its scratch directory; its ports and names free\nno previews\n",
            doc["name"].as_str().unwrap()
        )
    );
    let released = format!(
        "released {}\n",
        doc["names"]["SLUICE_NAME_WEB"].as_str().unwrap()
    );
    assert_eq!(std::fs::read_to_string(&s.log).unwrap(), released);
    assert!(s.held().is_empty());
    let out = s.sluice(&["previews"]);
    assert_eq!(text(&out), "no previews\n");
    assert_eq!(std::fs::read_to_string(&s.log).unwrap(), released);
}

/// 101, 103, S33: a command that fails is reported with what it printed,
/// released at once, and kept listed until released; `--wait` exits 1.
#[test]
fn a_failed_command_is_released_and_kept_listed() {
    let s = Setup::new("preview-failed", 42020, "echo cannot start; exit 3");
    let rev = s.repo.rev("bolt/x");
    let out = s.sluice(&["preview", "bolt/x", &rev, "--wait"]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    assert!(text(&out).contains("  | cannot start"), "{}", text(&out));
    assert!(s.held().is_empty());
    assert!(!s.repo.dir().join(".git/sluice/results.jsonl").exists());
    let listed = s.sluice(&["previews", "--json"]);
    let docs: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert_eq!(docs[0]["state"], "failed");
    let out = s.sluice(&["release", &rev]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(text(&s.sluice(&["previews"])), "no previews\n");
}

/// Record 0121's containers for the preview `doc`, its place's label
/// `place`: one mounting a file of its checkout, one publishing its `web`
/// port, one labelled `sluice.place` with `place`, and one of another
/// checkout that does none of these.
fn four(doc: &serde_json::Value, place: &str) -> serde_json::Value {
    let checkout = PathBuf::from(doc["checkout"].as_str().unwrap());
    let port: u16 = doc["ports"]["SLUICE_PORT_WEB"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let other = checkout.with_extension("other").join("tls");
    serde_json::json!([
        support::container("mounts", &[&checkout.join("tls/cert.pem")], &[], &[]),
        support::container("publishes", &[], &[port], &[]),
        support::container("labelled", &[], &[], &[("sluice.place", place)]),
        support::container("other", &[&other], &[port + 1], &[]),
    ])
}

/// Hold `containers` at `s`'s engine once its release log exists: an end
/// before the release command would find none to remove.
fn held_once_released(s: &Setup, containers: serde_json::Value) {
    let log = s.log.clone();
    s.engine.hold_with(move || match log.exists() {
        true => containers.clone(),
        false => serde_json::json!([]),
    });
}

/// 34, previews-1 clause 101, record 0121, the preview-release spec's
/// "The release command leaves a container" and "Another checkout's
/// container is left": `sluice release` runs the release command, then
/// removes the container that mounts the checkout, the one that
/// publishes its port and the one labelled with its place, and leaves the
/// fourth; the checkout is removed and the port freed.
#[test]
fn a_released_previews_container_ends_with_it() {
    let s = Setup::new(
        "preview-released-container",
        42030,
        "printf %s $SLUICE_PLACE > {place}",
    );
    let doc = s.preview();
    assert_eq!(doc["state"], "up", "{doc}");
    let place = std::fs::read_to_string(&s.place).unwrap();
    assert!(place.starts_with("sluice-"), "{place}");
    held_once_released(&s, four(&doc, &place));

    let out = s.sluice(&["release", doc["name"].as_str().unwrap()]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(
        std::fs::read_to_string(&s.log).unwrap(),
        format!(
            "released {}\n",
            doc["names"]["SLUICE_NAME_WEB"].as_str().unwrap()
        )
    );
    assert_eq!(s.engine.removed(), ["mounts", "publishes", "labelled"]);
    assert!(!PathBuf::from(doc["checkout"].as_str().unwrap()).exists());
    assert!(s.held().is_empty());
}

/// The release command of a preview whose script is in its checkout: it
/// logs that it was tried, then runs the script, which logs that it ran.
const SCRIPTED: &str = "echo tried >> {log}; sh infra/release.sh";

/// The script [`SCRIPTED`] runs, committed in the checkout.
const SCRIPT: (&str, &str) = ("infra/release.sh", "echo released >> {log}\n");

/// 34, 123, previews-1 clause 101, record 0121, the preview-release
/// spec's "A checkout removed by hand": the next `sluice previews` runs
/// the stored release command once in the scratch directory, where the
/// script in the checkout cannot be found and it fails, removes the
/// container that mounts a file of the checkout all the same, and frees
/// the preview; no later listing runs its release again.
#[test]
fn a_removed_checkouts_container_ends_with_its_preview() {
    let s = Setup::with("preview-gone-container", 42040, "true", SCRIPTED, &[SCRIPT]);
    let doc = s.preview();
    assert_eq!(doc["state"], "up", "{doc}");
    let name = doc["name"].as_str().unwrap();
    held_once_released(&s, four(&doc, "sluice-ffffffffff"));
    let checkout = doc["checkout"].as_str().unwrap();
    s.repo.git(&["worktree", "remove", "--force", checkout]);

    let out = s.sluice(&["previews"]);
    assert!(out.status.success(), "{}", text(&out));
    let said = text(&out);
    assert!(
        said.starts_with(&format!(
            "freed {name}: its checkout no longer exists; its release failed once from its scratch directory: "
        )),
        "{said}"
    );
    assert!(
        said.ends_with("its ports and names free\nno previews\n"),
        "{said}"
    );
    assert_eq!(std::fs::read_to_string(&s.log).unwrap(), "tried\n");
    assert_eq!(s.engine.removed(), ["mounts", "publishes"]);
    assert!(s.held().is_empty());
    assert_eq!(text(&s.sluice(&["previews"])), "no previews\n");
    assert_eq!(std::fs::read_to_string(&s.log).unwrap(), "tried\n");
}

/// 123, previews-1 clause 101, record 0121, the preview-release spec's
/// "A release failure accepted": a preview held by its failing release
/// command keeps its container; `--accept-failure` removes the container
/// that publishes its port, runs no release command and frees it.
#[test]
fn an_accepted_previews_container_ends_with_it() {
    let s = Setup::with(
        "preview-accepted-container",
        42050,
        "true",
        "echo released >> {log}; exit 1",
        &[],
    );
    let doc = s.preview();
    assert_eq!(doc["state"], "up", "{doc}");
    let name = doc["name"].as_str().unwrap();
    s.engine.hold(four(&doc, "sluice-ffffffffff"));

    let out = s.sluice(&["release", name]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    assert!(s.engine.removed().is_empty(), "removed before its release");
    let out = s.sluice(&["release", name, "--accept-failure"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(s.engine.removed(), ["mounts", "publishes"]);
    assert_eq!(std::fs::read_to_string(&s.log).unwrap(), "released\n");
    assert!(!PathBuf::from(doc["checkout"].as_str().unwrap()).exists());
    assert!(s.held().is_empty());
    assert_eq!(text(&s.sluice(&["previews"])), "no previews\n");
}

/// 101, 103, record 0121, the preview-release spec's "A failed command":
/// a command that starts a container labelled with its `SLUICE_PLACE`
/// and exits 1 is reported failed, its release command runs, and the
/// container is removed.
#[test]
fn a_failed_previews_labelled_container_ends_with_it() {
    let s = Setup::new(
        "preview-failed-container",
        42060,
        "printf %s $SLUICE_PLACE > {place}; exit 1",
    );
    let place = s.place.clone();
    s.engine
        .hold_with(move || match std::fs::read_to_string(&place) {
            Ok(label) => serde_json::json!([support::container(
                "labelled",
                &[],
                &[],
                &[("sluice.place", &label)]
            )]),
            Err(_) => serde_json::json!([]),
        });
    let rev = s.repo.rev("bolt/x");
    let out = s.sluice(&["preview", "bolt/x", &rev, "--wait"]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    assert!(text(&out).contains("failed"), "{}", text(&out));
    assert!(std::fs::read_to_string(&s.log)
        .unwrap()
        .starts_with("released "));
    assert_eq!(s.engine.removed(), ["labelled"]);
    assert!(s.held().is_empty());
}

/// Record 0121, the preview-release spec's "A host with no container
/// engine": where no engine answers, a preview is released as before.
#[test]
fn a_preview_on_a_host_with_no_engine_is_released_as_before() {
    let mut s = Setup::new("preview-no-engine", 42070, "true");
    s.sockets = s._engine_dir.path().join("none.sock").display().to_string();
    let doc = s.preview();
    assert_eq!(doc["state"], "up", "{doc}");
    assert_eq!(s.held().len(), 1);
    let out = s.sluice(&["release", doc["name"].as_str().unwrap()]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(!PathBuf::from(doc["checkout"].as_str().unwrap()).exists());
    assert!(s.held().is_empty());
    assert_eq!(s.engine.listings(), 0);
}

/// 123, record 0121, the preview-release spec's "The engine refuses a
/// removal": `sluice release` exits 1 naming the container and the
/// engine's words; the preview stays listed with its release's failure,
/// its port held.
#[test]
fn a_container_the_engine_will_not_remove_keeps_its_preview() {
    let s = Setup::new("preview-container-refused", 42080, "true");
    let doc = s.preview();
    assert_eq!(doc["state"], "up", "{doc}");
    let name = doc["name"].as_str().unwrap();
    s.engine.hold(four(&doc, "sluice-ffffffffff"));
    s.engine.refuse("publishes", 1);

    let out = s.sluice(&["release", name]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    let refused = format!("publishes: {}", support::REFUSED.1);
    assert!(text(&out).contains(&refused), "{}", text(&out));
    assert!(
        text(&out).contains(&format!("could not remove the containers {name} left")),
        "{}",
        text(&out)
    );
    assert_eq!(s.engine.removed(), ["mounts"], "the rest went on");
    let listed = s.sluice(&["previews", "--json"]);
    let docs: serde_json::Value =
        serde_json::from_slice(&listed.stdout).unwrap_or_else(|_| panic!("{}", text(&listed)));
    assert_eq!(docs[0]["name"], name, "{docs}");
    let failed = docs[0]["release_failed"]["failed"].as_str().unwrap_or("");
    assert!(failed.contains(&refused), "{docs}");
    assert_eq!(s.held().len(), 1, "its port stays held");
    assert!(PathBuf::from(doc["checkout"].as_str().unwrap()).exists());
}

/// 123, record 0121, the preview-release spec's "The engine removes it
/// on a later try": a gone preview whose container the engine refuses is
/// kept by the first `sluice previews`, which names what failed, and
/// freed by the second; its release ran once in all.
#[test]
fn a_gone_previews_refused_container_is_tried_again_by_the_next_listing() {
    let s = Setup::with(
        "preview-gone-container-refused",
        42090,
        "true",
        SCRIPTED,
        &[SCRIPT],
    );
    let doc = s.preview();
    assert_eq!(doc["state"], "up", "{doc}");
    let name = doc["name"].as_str().unwrap();
    s.engine.hold(four(&doc, "sluice-ffffffffff"));
    s.engine.refuse("mounts", 1);
    let checkout = doc["checkout"].as_str().unwrap();
    s.repo.git(&["worktree", "remove", "--force", checkout]);

    let out = s.sluice(&["previews"]);
    let said = text(&out);
    assert!(
        said.contains(&format!("could not release {name}: ")),
        "{said}"
    );
    assert!(
        said.contains(&format!("mounts: {}", support::REFUSED.1)),
        "{said}"
    );
    assert!(said.contains(name), "the preview stays listed: {said}");
    assert_eq!(s.held().len(), 1, "its port stays held");
    assert_eq!(s.engine.removed(), ["publishes"]);

    let out = s.sluice(&["previews"]);
    let said = text(&out);
    assert!(said.starts_with(&format!("freed {name}: ")), "{said}");
    assert!(said.ends_with("no previews\n"), "{said}");
    assert_eq!(s.engine.removed(), ["publishes", "mounts"]);
    assert!(s.held().is_empty());
    assert_eq!(std::fs::read_to_string(&s.log).unwrap(), "tried\n");
}
