//! `sluice preview`, `sluice previews` and `sluice release <preview>` over
//! the fakes (previews-1 clauses 98 to 103).

mod support;

use std::cell::Cell;

use clap::Parser;
use sluice::{run, Cli, Pause, CONFLICT, FAILED, LANDED, STOPPED};
use sluice_fakes::Fakes;

const BRANCH: &str = "bolt/concurrent-units";

const POLICY: &str = "[checkout]\nports = [\"web\", \"api\"]\n\n\
[branches.\"bolt/*\"]\nrequired = [\"#gate\"]\nparallel = 4\n\n\
[preview]\ncommand = [\"moon\", \"run\", \"app:preview\"]\n\
addresses = { web = \"https://{name.web}.localhost\", api = \"http://localhost:{port.api}/docs\" }\n";

struct Then<F: Fn(usize) -> bool>(Cell<usize>, F);

impl<F: Fn(usize) -> bool> Pause for Then<F> {
    fn pause(&self) -> bool {
        self.0.set(self.0.get() + 1);
        assert!(self.0.get() < 50, "waited too long");
        (self.1)(self.0.get())
    }
}

fn setup() -> Fakes {
    let f = Fakes::new();
    f.world.init(
        BRANCH,
        &[
            (".config/sluice.toml", POLICY),
            ("plugin/lib/plan.py", "plan"),
        ],
    );
    f.world.task("plugin:test", "plugin/");
    f
}

fn sluice_with(f: &Fakes, reserved: usize, pause: &dyn Pause, args: &[&str]) -> (i32, String) {
    let cli = Cli::try_parse_from(std::iter::once("sluice").chain(args.iter().copied())).unwrap();
    let mut out = Vec::new();
    let host = support::FakeHost::new(f.world.clone(), reserved);
    let code = run(cli, f.seams(), &host, &(), &(), &f.clock, pause, &mut out).unwrap();
    (code, String::from_utf8(out).unwrap())
}

fn sluice(f: &Fakes, args: &[&str]) -> (i32, String) {
    sluice_with(f, 0, &Then(Cell::new(0), |_| true), args)
}

/// 35, 98: a preview's command is given the host's caches as any run is:
/// the binary on a scratch repository, its host's config declaring one
/// cache's variable.
#[test]
fn a_preview_gets_the_hosts_cache_environment() {
    let repo = support::Repo::new("preview-cache-env");
    let seen = repo.dir().with_extension("seen");
    repo.write(
        ".config/sluice.toml",
        &format!(
            "[tests]\nrunner = \"command\"\ncommand = \"true\"\n\n\
             [branches.\"bolt/*\"]\ntimeout = \"1m\"\n\n\
             [preview]\ncommand = \"env > {}\"\n",
            seen.display()
        ),
    );
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "a preview"]);
    repo.git(&["branch", "bolt/x", "main"]);
    let config = support::Scratch::new("preview-cache-env-config");
    let state = support::Scratch::new("preview-cache-env-state");
    std::fs::create_dir_all(config.path().join("sluice")).unwrap();
    std::fs::write(
        config.path().join("sluice/config.toml"),
        format!(
            "[[caches]]\ndir = {:?}\nenv = {{ SCCACHE_DIR = \"/caches/sccache\" }}\n",
            config.path().join("cache").display().to_string()
        ),
    )
    .unwrap();
    // Record 0121: its release ends its containers at this engine of its
    // own, never the host's.
    let engine_dir = support::Scratch::short();
    let engine = support::containers_engine(engine_dir.path());
    let sluice = |args: &[&str]| {
        support::isolated(&mut std::process::Command::new(env!(
            "CARGO_BIN_EXE_sluice"
        )))
        .args(args)
        .current_dir(repo.dir())
        .env("XDG_CONFIG_HOME", config.path())
        .env("XDG_STATE_HOME", state.path())
        .env(
            sluice::profiles::checks::ENGINE_SOCKETS_VAR,
            engine.socket(),
        )
        .env_remove("SLUICE_AGENT")
        .env_remove("SLUICE_OTEL")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap()
    };
    let out = sluice(&["preview", "bolt/x", &repo.rev("bolt/x"), "--wait", "--json"]);
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&out.stderr)));
    assert_eq!(doc["state"], "up", "{doc}");
    let env = std::fs::read_to_string(&seen).unwrap();
    assert!(
        env.lines().any(|l| l == "SCCACHE_DIR=/caches/sccache"),
        "{env}"
    );
    let out = sluice(&["release", doc["name"].as_str().unwrap()]);
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(engine.listings(), 1, "its release asked its own engine");
}

/// The arguments: a branch and a revision; `previews`; `release` with a
/// preview or none.
#[test]
fn the_arguments() {
    assert!(Cli::try_parse_from(["sluice", "preview", BRANCH]).is_err());
    assert!(Cli::try_parse_from(["sluice", "preview", BRANCH, "abc", "--wait", "--json"]).is_ok());
    assert!(Cli::try_parse_from(["sluice", "previews", "--json"]).is_ok());
    assert!(Cli::try_parse_from(["sluice", "release", "preview/abc"]).is_ok());
    assert!(Cli::try_parse_from(["sluice", "release", "--worktree", "w"]).is_ok());
}

/// 98, 99, S30: the addresses are printed when `sluice preview` returns,
/// with the command started and not yet ended.
#[test]
fn a_preview_returns_its_addresses_once_its_command_starts() {
    let f = setup();
    f.world.hold_runs(true);
    let rev = f.world.tip(BRANCH);
    let (code, out) = sluice(&f, &["preview", BRANCH, rev.as_str()]);
    assert_eq!(code, LANDED, "{out}");
    let name = format!("preview/{}", &rev.as_str()[..12]);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        lines[0],
        format!("{name} of {BRANCH} at {}: starting", rev.short())
    );
    let p = &f.world.live_previews()[0];
    assert_eq!(
        lines[1],
        format!(
            "  api  http://localhost:{}/docs",
            p.ports["SLUICE_PORT_API"]
        )
    );
    assert_eq!(
        lines[2],
        format!("  web  https://{}.localhost", p.names["SLUICE_NAME_WEB"])
    );
    assert!(lines[3].starts_with("  ports: SLUICE_PORT_API="), "{out}");
    let req = f.world.requests().pop().unwrap();
    assert_eq!(req.place.name, name);
    assert_eq!(req.command.as_deref(), Some("moon run app:preview"));
    // 147: `[tests]` is the policy's, which names no runner here.
    assert!(!req.tests.contains_key("runner") && !req.tests.contains_key("command"));
    assert_eq!(req.place.env["SLUICE_NAME_WEB"], p.names["SLUICE_NAME_WEB"]);
    // 102: counted while it runs.
    assert_eq!(sluice_seams::Tests::running(&f.tests).unwrap().sessions, 1);
    f.world.complete(&rev);
    let (_, out) = sluice(&f, &["previews"]);
    assert!(
        out.starts_with(&format!("{name} of {BRANCH} at {}: up\n", rev.short())),
        "{out}"
    );
    assert_eq!(
        sluice_seams::Tests::running(&f.tests).unwrap().sessions,
        0,
        "an ended command counts against nothing"
    );
}

/// S35: a revision the branch has not landed is refused, and nothing is
/// made.
#[test]
fn an_unlanded_revision_is_refused() {
    let f = setup();
    let head = f
        .world
        .work(&f.world.tip(BRANCH), &[("plugin/x", Some("x"))]);
    let (code, out) = sluice(&f, &["preview", BRANCH, head.as_str()]);
    assert_eq!(code, CONFLICT);
    assert_eq!(
        out,
        format!(
            "refused: {BRANCH} has not landed {}: only what a branch has landed is previewed\n",
            head.short()
        )
    );
    assert!(f.world.live_previews().is_empty() && f.world.requests().is_empty());
    let (code, out) = sluice(&f, &["preview", "bolt/none", head.as_str()]);
    assert_eq!(
        (code, out.as_str()),
        (CONFLICT, "refused: no branch bolt/none\n")
    );
}

/// 100, S32: asking again gives the same preview and starts nothing.
#[test]
fn asking_again_gives_the_same_preview() {
    let f = setup();
    let rev = f.world.tip(BRANCH);
    let (_, first) = sluice(&f, &["preview", BRANCH, rev.as_str(), "--json"]);
    let (code, again) = sluice(&f, &["preview", BRANCH, rev.as_str(), "--json"]);
    assert_eq!(code, LANDED);
    let a: serde_json::Value = serde_json::from_str(&first).unwrap();
    let b: serde_json::Value = serde_json::from_str(&again).unwrap();
    assert_eq!(a, b);
    assert_eq!(a["state"], "up");
    assert_eq!(a["exit"], 0);
    assert_eq!(f.world.requests().len(), 1);
}

/// 102: with the bound full, the preview waits and starts when a run
/// ends; it never takes the reserve.
#[test]
fn a_preview_waits_for_room_and_never_takes_the_reserve() {
    let f = setup();
    f.world.host_limit(2);
    f.world.session_run("/w/other", true);
    let rev = f.world.tip(BRANCH);
    let world = f.world.clone();
    let pause = Then(Cell::new(0), move |n| {
        if n == 1 {
            assert!(world.started().is_empty(), "started into the reserve");
            world.kill_session("/w/other");
        }
        true
    });
    let (code, out) = sluice_with(&f, 1, &pause, &["preview", BRANCH, rev.as_str()]);
    assert_eq!(code, LANDED, "{out}");
    assert!(
        out.starts_with("waiting: the host is at its bound\n"),
        "{out}"
    );
    assert_eq!(f.world.started().len(), 1);
}

/// 71, 102: a start the check seam refuses at the host's bound waits as
/// one the count refused, and starts on the next try.
#[test]
fn a_preview_waits_on_a_start_refused_at_the_bound() {
    let f = setup();
    let rev = f.world.tip(BRANCH);
    f.world.refuse_next_start();
    let (code, out) = sluice_with(
        &f,
        0,
        &Then(Cell::new(0), |_| true),
        &["preview", BRANCH, rev.as_str()],
    );
    assert_eq!(code, LANDED, "{out}");
    assert!(
        out.starts_with("waiting: the host is at its bound\n"),
        "{out}"
    );
    assert_eq!(f.world.started().len(), 1);
}

/// 102: stopped while it waits, nothing ran; asked again, the same
/// preview is taken up.
#[test]
fn a_preview_stopped_while_waiting_is_taken_up_again() {
    let f = setup();
    f.world.host_limit(1);
    f.world.session_run("/w/other", true);
    let rev = f.world.tip(BRANCH);
    let (code, out) = sluice_with(
        &f,
        0,
        &Then(Cell::new(0), |_| false),
        &["preview", BRANCH, rev.as_str()],
    );
    assert_eq!(code, STOPPED, "{out}");
    assert!(f.world.started().is_empty());
    let (_, out) = sluice(&f, &["previews"]);
    assert!(out.contains(": waiting\n"), "{out}");
    let names = f.world.live_previews()[0].names.clone();
    f.world.kill_session("/w/other");
    let (code, out) = sluice(&f, &["preview", BRANCH, rev.as_str()]);
    assert_eq!(code, LANDED, "{out}");
    assert_eq!(f.world.started().len(), 1);
    assert_eq!(f.world.live_previews()[0].names, names);
}

/// 101, 103, S33: a failed command is reported with what it printed and
/// released; `--wait` exits 1; nothing is recorded; it stays listed until
/// released.
#[test]
fn a_failed_preview_is_reported_and_released() {
    let f = setup();
    f.world.preview_fails(true);
    let rev = f.world.tip(BRANCH);
    let (code, out) = sluice(&f, &["preview", BRANCH, rev.as_str(), "--wait"]);
    assert_eq!(code, FAILED, "{out}");
    let name = format!("preview/{}", &rev.as_str()[..12]);
    assert!(
        out.starts_with(&format!(
            "released {name}: its command failed; `sluice previews` shows what it printed\n"
        )),
        "{out}"
    );
    assert!(
        out.contains(": failed\n") && out.contains("\n  | preview failed"),
        "{out}"
    );
    assert_eq!(f.world.results_log(), "");
    assert_eq!(f.world.preview_releases(), vec![(name.clone(), true)]);
    let (_, listed) = sluice(&f, &["previews", "--json"]);
    let docs: serde_json::Value = serde_json::from_str(&listed).unwrap();
    assert_eq!(docs[0]["state"], "failed");
    assert!(docs[0]["output"].as_str().unwrap().contains("failed"));
    let (code, out) = sluice(&f, &["release", &name]);
    assert_eq!(code, LANDED);
    assert!(out.starts_with(&format!("released {name}: ")), "{out}");
    assert_eq!(sluice(&f, &["previews"]).1, "no previews\n");
    assert_eq!(sluice(&f, &["release", &name]).1, "nothing to release\n");
    let (code, out) = sluice(&f, &["release", &name, "--json"]);
    assert_eq!(code, LANDED);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&out).unwrap(),
        serde_json::json!({"preview": name, "released": false})
    );
}

/// 101: released by its revision; a preview that is up is never released
/// by a step.
#[test]
fn a_preview_is_released_only_when_asked() {
    let f = setup();
    let rev = f.world.tip(BRANCH);
    sluice(&f, &["preview", BRANCH, rev.as_str()]);
    let (code, _) = sluice(&f, &["step", BRANCH]);
    assert_eq!(code, 0);
    assert!(f.world.preview_releases().is_empty());
    let (_, out) = sluice(&f, &["release", rev.as_str(), "--json"]);
    let doc: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(
        doc["preview"].as_str().unwrap().starts_with("preview/"),
        "{out}"
    );
    assert_eq!(doc["released"], true);
    assert!(f.world.live_previews().is_empty());
}
