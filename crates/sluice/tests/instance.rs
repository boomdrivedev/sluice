//! The instance's bound on real places: section 4's acceptance test 7 in
//! `docs/plans/phase-1-integrations.md` (71; ADR 0012). Three branches,
//! each with room for four candidates, are served by one `sluice run`
//! with `[runs] max = 2`; the check counts the runs alive as it starts
//! and the cores it was given. The policy names no `trust`, so the
//! default lands what the open executor passed (ADR 0015). That test
//! needs git, so it is in the check-tools tier; the reading of the
//! host's settings needs none.

#[path = "support/mod.rs"]
mod support;

use std::path::Path;

use sluice::instance::Instance;
use support::Scratch;

fn read(text: &str) -> anyhow::Result<Instance> {
    let dir = Scratch::new("instance-read");
    let path = dir.path().join("config.toml");
    std::fs::write(&path, text).unwrap();
    Instance::from_file(Some(&path), 16)
}

/// Session-runs-1 clause 91: with no `reserved`, session runs may hold
/// the whole bound.
#[test]
fn the_reserve_is_none_by_default() {
    assert_eq!(read("[runs]\nmax = 4\n").unwrap().reserved, 0);
    assert_eq!(Instance::from_file(None, 16).unwrap().reserved, 0);
    assert_eq!(
        Instance::from_file(Some(Path::new("/nonexistent/sluice.toml")), 16)
            .unwrap()
            .reserved,
        0
    );
}

/// 91: the host keeps `reserved` runs of its bound for submissions' runs.
#[test]
fn the_reserve_is_read_beside_the_bound() {
    let i = read("[runs]\nmax = 4\nreserved = 1\n").unwrap();
    assert_eq!((i.bound, i.reserved), (4, 1));
    // Beside the default bound, a quarter of the cores.
    assert_eq!(read("[runs]\nreserved = 3\n").unwrap().reserved, 3);
}

/// 91: a reserve at or above the bound leaves session runs nothing, and
/// is refused with the setting named.
#[test]
fn a_reserve_that_leaves_no_run_is_refused() {
    let e = read("[runs]\nmax = 2\nreserved = 2\n")
        .unwrap_err()
        .to_string();
    assert_eq!(
        e,
        "[runs] reserved = 2 leaves no run of [runs] max = 2 for session runs: \
         it must be less than the bound"
    );
    assert!(read("[runs]\nmax = 2\nreserved = 3\n").is_err());
    let e = read("[runs]\nmax = 2\nreserved = -1\n")
        .unwrap_err()
        .to_string();
    assert!(e.contains("[runs] reserved"), "{e}");
}

/// 124: branch protection reads only `[forge]` of the host's settings,
/// passing over a table, a field and a field of `[forge]` no sluice yet
/// reads, each of which every other command refuses.
#[test]
fn guarding_reads_only_the_forge() {
    use std::os::unix::fs::PermissionsExt;
    let dir = Scratch::new("instance-guarding");
    let key = dir.path().join("app.pem");
    std::fs::write(&key, "key").unwrap();
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        format!(
            "[runs]\ncores = 12\n\n[later]\nx = 1\n\n[forge]\napp_id = 42\nkey = {:?}\nlater = 1\n",
            key.display().to_string()
        ),
    )
    .unwrap();
    let i = Instance::guarding_from(Some(&path)).unwrap();
    assert_eq!(
        i.forge,
        Some(sluice::instance::Forge {
            app_id: 42,
            source: sluice::instance::Source::Key(key.clone()),
            discovery: sluice::instance::DISCOVERY,
        })
    );
    assert_eq!((i.signing, i.bound), (None, 1));
    let e = format!("{:#}", Instance::from_file(Some(&path), 16).unwrap_err());
    assert!(e.contains(&path.display().to_string()), "{e}");

    // A forge guarding cannot read is an error naming the file.
    std::fs::write(&path, "[forge]\napp_id = \"42\"\nkey = \"k\"\n").unwrap();
    let e = format!("{:#}", Instance::guarding_from(Some(&path)).unwrap_err());
    assert!(e.contains(&path.display().to_string()), "{e}");
    std::fs::write(&path, "[forge]\nkey = \"k\"\n").unwrap();
    assert!(Instance::guarding_from(Some(&path)).is_err());

    // Settings that are not TOML are read by neither.
    std::fs::write(&path, "[runs\nnot toml").unwrap();
    let e = format!("{:#}", Instance::guarding_from(Some(&path)).unwrap_err());
    assert!(e.contains(&path.display().to_string()), "{e}");
    assert!(Instance::from_file(Some(&path), 16).is_err());

    // No file, no forge.
    assert_eq!(
        Instance::guarding_from(Some(&dir.path().join("none.toml")))
            .unwrap()
            .forge,
        None
    );
}

/// 136: a branch's open pull requests are asked for once a minute where
/// the host sets no `[forge] discovery`, at the interval it sets, and
/// never more often than every ten seconds.
#[test]
fn discovery_is_read_from_the_forge() {
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;
    let dir = Scratch::new("instance-discovery");
    let key = dir.path().join("app.pem");
    std::fs::write(&key, "key").unwrap();
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
    let forge = |lines: &str| {
        read(&format!(
            "[forge]\napp_id = 42\nkey = {:?}\n{lines}",
            key.display().to_string()
        ))
    };
    let discovery = |lines: &str| forge(lines).unwrap().forge.unwrap().discovery;
    assert_eq!(discovery(""), Duration::from_secs(60));
    assert_eq!(discovery("discovery = \"2m\"\n"), Duration::from_secs(120));
    assert_eq!(discovery("discovery = \"10s\"\n"), Duration::from_secs(10));
    let e = format!("{:#}", forge("discovery = \"5s\"\n").unwrap_err());
    assert!(
        e.contains("[forge] discovery = \"5s\" is shorter than ten seconds"),
        "{e}"
    );
    assert!(forge("discovery = \"soon\"\n").is_err());
}

#[cfg(feature = "tools")]
mod bound {
    use super::support;

    use std::path::{Path, PathBuf};
    use std::process::{Command, Output, Stdio};
    use std::rc::Rc;

    use support::{Repo, Scratch};

    const BRANCHES: [&str; 3] = ["bolt/a", "bolt/b", "bolt/c"];

    const POLICY: &str = "[tests]\nrunner = \"command\"\ncommand = \"sh check.sh\"\n\n\
    [branches.\"bolt/*\"]\nparallel = 4\ntimeout = \"1m\"\n";

    /// Each run marks itself alive in `alive/`, writes how many runs were
    /// alive as it started and the `SLUICE_JOBS` it was given, holds until
    /// another run is alive or 6 s have passed, so the others started in
    /// the same round start beside it even on a loaded host, and leaves.
    fn check(seen: &Path) -> String {
        check_until(seen, 2)
    }

    /// `check`, holding until `alive` runs are alive or 6 s have passed.
    fn check_until(seen: &Path, alive: usize) -> String {
        check_holding(seen, alive, 30)
    }

    /// `check`, holding until `alive` runs are alive or `tries` fifths of
    /// a second have passed.
    fn check_holding(seen: &Path, alive: usize, tries: usize) -> String {
        let seen = seen.display();
        format!(
            "mkdir -p {seen}/alive\n\
             touch {seen}/alive/$$\n\
             echo \"$(ls {seen}/alive | wc -l | tr -d ' ') $SLUICE_JOBS\" >> {seen}/starts\n\
             i=0; while [ \"$(ls {seen}/alive | wc -l | tr -d ' ')\" -lt {alive} ] && [ $i -lt {tries} ]; do sleep 0.2; i=$((i+1)); done\n\
             sleep 1\n\
             rm {seen}/alive/$$\n"
        )
    }

    /// A check whose first run writes `lines` over the host's settings at
    /// `config` once, behind a marker in `seen`, by a rename as an editor
    /// writes a file; every run is then `check_holding`'s, until two runs
    /// are alive or `tries` fifths of a second have passed.
    fn check_writing(seen: &Path, config: &Path, lines: &[&str], tries: usize) -> String {
        let quoted: Vec<String> = lines.iter().map(|l| format!("'{l}'")).collect();
        format!(
            "if mkdir {seen}/wrote 2>/dev/null; then \
             printf '%s\\n' {lines} > {config}.next && mv {config}.next {config}; fi\n{check}",
            seen = seen.display(),
            config = config.display(),
            lines = quoted.join(" "),
            check = check_holding(seen, 2, tries),
        )
    }

    /// What `check` wrote of each run: how many were alive as it started,
    /// and the `SLUICE_JOBS` it was given.
    fn starts(seen: &Path) -> Vec<(usize, usize)> {
        std::fs::read_to_string(seen.join("starts"))
            .unwrap()
            .lines()
            .map(|l| {
                let (alive, jobs) = l.split_once(' ').expect("alive and jobs");
                (alive.parse().unwrap(), jobs.parse().unwrap())
            })
            .collect()
    }

    /// The host's configuration: a bound of `max`.
    fn host_config(max: usize) -> Scratch {
        let config = Scratch::new("instance-config");
        std::fs::create_dir_all(config.path().join("sluice")).unwrap();
        std::fs::write(
            config.path().join("sluice/config.toml"),
            format!("[runs]\nmax = {max}\n"),
        )
        .unwrap();
        config
    }

    fn text(out: &Output) -> String {
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    }

    struct Setup {
        repo: Repo,
        sessions: Scratch,
        config: Rc<Scratch>,
        /// The host's state of its own: a session run alive on the host
        /// running the test, its own `sluice exec` included, would count
        /// toward the bound (rule core.18) and hold the runs to one.
        state: Rc<Scratch>,
    }

    impl Setup {
        fn command(&self, dir: &Path) -> Command {
            let mut c = Command::new(env!("CARGO_BIN_EXE_sluice"));
            support::isolated(&mut c)
                .current_dir(dir)
                .env("XDG_CONFIG_HOME", self.config.path())
                .env("XDG_STATE_HOME", self.state.path())
                .env_remove("SLUICE_AGENT")
                .env_remove("SLUICE_OTEL")
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .stdin(Stdio::null());
            c
        }

        /// A session on `unit/<name>` off `branch`, with one commit of its
        /// own file, submitted without waiting.
        fn submit(&self, branch: &str, name: &str) {
            let dir: PathBuf = self.sessions.path().join(name);
            self.repo.git(&[
                "worktree",
                "add",
                "-q",
                "-b",
                &format!("unit/{name}"),
                dir.to_str().unwrap(),
                branch,
            ]);
            std::fs::write(dir.join(format!("{name}.txt")), name).unwrap();
            let git = |args: &[&str]| {
                let mut a = vec!["-C", dir.to_str().unwrap()];
                a.extend(args);
                self.repo.git(&a)
            };
            git(&["add", "-A"]);
            git(&["commit", "-q", "-m", name]);
            let out = self
                .command(&dir)
                .args(["submit", branch, "--no-wait", "--json"])
                .output()
                .unwrap();
            assert!(out.status.success(), "{}", text(&out));
        }
    }

    /// 71: with `[runs] max = 2`, one `sluice run` serving three branches
    /// with a window of four each has at most two runs under test at once,
    /// reaches two, and gives each run half the cores.
    #[test]
    fn one_run_holds_three_branches_to_its_bound() {
        let repo = Repo::new("instance-bound");
        let seen = Scratch::new("instance-seen");
        repo.write(".config/sluice.toml", POLICY);
        repo.write("check.sh", &check(seen.path()));
        repo.git(&["add", "-A"]);
        repo.git(&["commit", "-q", "-m", "sluice's policy"]);
        for b in BRANCHES {
            repo.git(&["branch", b, "main"]);
        }
        let s = Setup {
            repo,
            sessions: Scratch::new("instance-sessions"),
            config: Rc::new(host_config(2)),
            state: Rc::new(Scratch::new("instance-state")),
        };
        // Two submissions on each branch: six candidates want a run at once.
        for (n, b) in BRANCHES.iter().enumerate() {
            s.submit(b, &format!("s{n}a"));
            s.submit(b, &format!("s{n}b"));
        }

        let out = s
            .command(s.repo.dir())
            .args(["run", "--rounds", "16"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", text(&out));

        let starts = starts(seen.path());
        assert_eq!(starts.len(), 6, "every candidate ran once: {starts:?}");
        let most = starts.iter().map(|(a, _)| *a).max().unwrap();
        assert_eq!(most, 2, "runs alive at once: {starts:?}");
        let processors = std::thread::available_parallelism().map_or(1, |n| n.get());
        for (_, jobs) in &starts {
            assert_eq!(*jobs, (processors / 2).max(1), "{starts:?}");
        }
        for b in BRANCHES {
            let out = s
                .command(s.repo.dir())
                .args(["status", b, "--json", "--all"])
                .output()
                .unwrap();
            let status: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
            let states: Vec<&str> = status["submissions"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x["state"].as_str().unwrap())
                .collect();
            assert_eq!(states, ["landed", "landed"], "{b}: {status}");
        }
    }

    /// The statuses of `branch`'s submissions in `s`'s repository.
    fn states(s: &Setup, branch: &str) -> Vec<String> {
        let out = s
            .command(s.repo.dir())
            .args(["status", branch, "--json", "--all"])
            .output()
            .unwrap();
        let status: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        status["submissions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x["state"].as_str().unwrap().to_string())
            .collect()
    }

    /// 71, S60: three repositories on one host whose bound is three, with
    /// a `sluice run` going in each at once and four combinations ready
    /// across them: at most three run at once, three do, the fourth runs
    /// once one has ended, all four land, and every run is given the
    /// host's cores divided by three.
    #[test]
    fn three_repositories_share_the_hosts_bound() {
        let seen = Scratch::new("instance-three-seen");
        let config = Rc::new(host_config(3));
        let state = Rc::new(Scratch::new("instance-three-state"));
        let setups: Vec<Setup> = ["x", "y", "z"]
            .iter()
            .map(|name| {
                let repo = Repo::new(&format!("instance-three-{name}"));
                repo.write(".config/sluice.toml", POLICY);
                repo.write("check.sh", &check_until(seen.path(), 3));
                repo.git(&["add", "-A"]);
                repo.git(&["commit", "-q", "-m", "sluice's policy"]);
                repo.git(&["branch", "bolt/a", "main"]);
                Setup {
                    repo,
                    sessions: Scratch::new(&format!("instance-three-{name}-sessions")),
                    config: config.clone(),
                    state: state.clone(),
                }
            })
            .collect();
        // Four combinations ready at once: two in the first repository.
        setups[0].submit("bolt/a", "xa");
        setups[0].submit("bolt/a", "xb");
        setups[1].submit("bolt/a", "ya");
        setups[2].submit("bolt/a", "za");

        let runs: Vec<_> = setups
            .iter()
            .map(|s| {
                s.command(s.repo.dir())
                    .args(["run", "--rounds", "16"])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap()
            })
            .collect();
        for run in runs {
            let out = run.wait_with_output().unwrap();
            assert!(out.status.success(), "{}", text(&out));
        }

        let starts = starts(seen.path());
        assert_eq!(starts.len(), 4, "every combination ran once: {starts:?}");
        let most = starts.iter().map(|(a, _)| *a).max().unwrap();
        assert_eq!(most, 3, "runs alive at once: {starts:?}");
        let processors = std::thread::available_parallelism().map_or(1, |n| n.get());
        for (_, jobs) in &starts {
            assert_eq!(*jobs, (processors / 3).max(1), "{starts:?}");
        }
        assert_eq!(states(&setups[0], "bolt/a"), ["landed", "landed"]);
        assert_eq!(states(&setups[1], "bolt/a"), ["landed"]);
        assert_eq!(states(&setups[2], "bolt/a"), ["landed"]);
    }

    /// One branch, `bolt/a`, testing four at once, under a host bound of
    /// one, its check `check`, with two submissions queued: the setup and
    /// the host's settings file.
    fn two_queued(name: &str, check: impl FnOnce(&Path) -> String) -> (Setup, PathBuf) {
        let repo = Repo::new(&format!("instance-{name}"));
        let config = Rc::new(host_config(1));
        let file = config.path().join("sluice/config.toml");
        repo.write(".config/sluice.toml", POLICY);
        repo.write("check.sh", &check(&file));
        repo.git(&["add", "-A"]);
        repo.git(&["commit", "-q", "-m", "sluice's policy"]);
        repo.git(&["branch", "bolt/a", "main"]);
        let s = Setup {
            repo,
            sessions: Scratch::new(&format!("instance-{name}-sessions")),
            config,
            state: Rc::new(Scratch::new(&format!("instance-{name}-state"))),
        };
        s.submit("bolt/a", "ta");
        s.submit("bolt/a", "tb");
        (s, file)
    }

    /// What sluice's host process said in `s`'s state, its log and the
    /// log of the process before it.
    fn host_log(s: &Setup) -> String {
        let host = s.state.path().join("sluice/host");
        ["log.1", "log"]
            .iter()
            .map(|f| std::fs::read_to_string(host.join(f)).unwrap_or_default())
            .collect()
    }

    /// The settings-take-up spec's "The bound grows while the host process
    /// runs", "The first run keeps its cores" and "The host process says
    /// what it took up" (135; record 0107; rule core.54; design D19): the
    /// bound written from one to two while the first combination's run
    /// goes is taken up by sluice's host process between its passes, and
    /// its next pass's run starts beside the first with half the cores,
    /// while the first keeps all of them; both land, and the take-up is
    /// said once in its log. Each run holds for up to twelve seconds, so
    /// the second starts beside the first even on a loaded host.
    #[test]
    fn the_host_process_takes_up_a_changed_bound_at_its_next_pass() {
        let seen = Scratch::new("instance-take-up-seen");
        let (s, file) = two_queued("take-up", |file| {
            check_writing(seen.path(), file, &["[runs]", "max = 2"], 60)
        });

        let out = s
            .command(s.repo.dir())
            .args(["run", "--rounds", "12"])
            .output()
            .unwrap();
        let said = text(&out);
        assert!(out.status.success(), "{said}");

        let processors = std::thread::available_parallelism().map_or(1, |n| n.get());
        let log = host_log(&s);
        assert_eq!(
            starts(seen.path()),
            [(1, processors), (2, (processors / 2).max(1))],
            "{said}{log}"
        );
        assert_eq!(states(&s, "bolt/a"), ["landed", "landed"], "{said}{log}");
        let took = format!(
            "sluice: took up the host's settings in {}: [runs] changed",
            file.display()
        );
        assert_eq!(log.matches(&took).count(), 1, "{log}");
        assert_eq!(
            log.matches("took up the host's settings in").count(),
            1,
            "{log}"
        );
    }

    /// The settings-take-up spec's "A key that holds no private key" (135;
    /// record 0107; rule core.54): settings that read but do not bind leave
    /// the run on the bound of one it holds, so the second combination's
    /// run starts only once the first has ended, with the cores the first
    /// had; both land, and the refusal is said once, naming the file and
    /// the key, with nothing taken up.
    #[test]
    fn a_run_keeps_its_settings_when_the_new_ones_do_not_bind() {
        use std::os::unix::fs::PermissionsExt;
        let seen = Scratch::new("instance-unbound-seen");
        let keys = Scratch::new("instance-unbound-key");
        let key = keys.path().join("app.pem");
        std::fs::write(&key, "no private key\n").unwrap();
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
        let key_line = format!("key = {:?}", key.display().to_string());
        let (s, file) = two_queued("unbound", |file| {
            check_writing(
                seen.path(),
                file,
                &["[runs]", "max = 2", "", "[forge]", "app_id = 1", &key_line],
                30,
            )
        });

        let out = s
            .command(s.repo.dir())
            .args(["run", "--rounds", "12"])
            .output()
            .unwrap();
        let said = text(&out);
        assert!(out.status.success(), "{said}");

        let processors = std::thread::available_parallelism().map_or(1, |n| n.get());
        assert_eq!(
            starts(seen.path()),
            [(1, processors), (1, processors)],
            "{said}"
        );
        // A command reads the file as it starts, and refuses a key that
        // does not read: the status is read under the settings the run held.
        std::fs::write(&file, "[runs]\nmax = 1\n").unwrap();
        assert_eq!(states(&s, "bolt/a"), ["landed", "landed"], "{said}");
        let refused: Vec<&str> = said
            .lines()
            .filter(|l| l.contains("is refused, so this sluice run keeps the settings it holds"))
            .collect();
        assert_eq!(refused.len(), 1, "{said}");
        assert!(refused[0].contains(&file.display().to_string()), "{said}");
        assert!(refused[0].contains(&key.display().to_string()), "{said}");
        assert!(!said.contains("took up"), "{said}");
    }

    /// The settings-take-up spec's "A kept run sends to the collector it
    /// is changed to" (135; 151; record 0107; rules core.53, core.54;
    /// design D1 to D7): `[telemetry]` changed to another collector while
    /// the first combination's run goes is taken up by the run starting
    /// itself afresh, which says so once; the first collector is sent what
    /// the run held before it, traces and metrics, and the second the
    /// traces of the run started afresh; each run starts once with every
    /// core, and both land.
    #[test]
    fn a_run_takes_a_changed_telemetry_table_up_by_starting_afresh() {
        use std::collections::BTreeSet;
        let ok = |_: &support::http::Request| (200, Vec::new(), String::new());
        let first = support::http::Fake::start(ok);
        let second = support::http::Fake::start(ok);
        let seen = Scratch::new("instance-afresh-seen");
        let sending = |collector: &str| {
            vec![
                "[runs]".to_string(),
                "max = 1".to_string(),
                String::new(),
                "[telemetry]".to_string(),
                "send = true".to_string(),
                format!("collector = \"{collector}\""),
            ]
        };
        let changed = sending(second.url());
        let (s, file) = two_queued("afresh", |file| {
            let lines: Vec<&str> = changed.iter().map(String::as_str).collect();
            check_writing(seen.path(), file, &lines, 30)
        });
        std::fs::write(&file, sending(first.url()).join("\n") + "\n").unwrap();

        // The caller's OpenTelemetry variables turn a signal off whatever
        // the table says (core.53), and `sluice exec` hands its command
        // those of the host's sluice: the run is given none.
        let mut run = s.command(s.repo.dir());
        for (name, _) in std::env::vars_os() {
            if name.to_string_lossy().starts_with("OTEL_") {
                run.env_remove(name);
            }
        }
        let out = run.args(["run", "--rounds", "12"]).output().unwrap();
        let said = text(&out);
        assert!(out.status.success(), "{said}");

        // Read before any other command sends to the second collector.
        let paths = |c: &support::http::Fake| -> BTreeSet<String> {
            c.requests().into_iter().map(|r| r.path).collect()
        };
        let (to_first, to_second) = (paths(&first), paths(&second));
        assert!(to_first.contains("/v1/traces"), "{to_first:?}\n{said}");
        assert!(to_first.contains("/v1/metrics"), "{to_first:?}\n{said}");
        assert!(to_second.contains("/v1/traces"), "{to_second:?}\n{said}");

        let processors = std::thread::available_parallelism().map_or(1, |n| n.get());
        assert_eq!(
            starts(seen.path()),
            [(1, processors), (1, processors)],
            "{said}"
        );
        assert_eq!(states(&s, "bolt/a"), ["landed", "landed"], "{said}");
        let took = format!(
            "sluice: took up the host's settings in {}: [telemetry] changed, in this sluice run \
             started afresh",
            file.display()
        );
        assert_eq!(said.matches(&took).count(), 1, "{said}");
        assert!(!said.contains("could not start afresh"), "{said}");
    }

    /// The settings-take-up spec's "Settings it would start afresh for that
    /// do not bind" (135; record 0107; rule core.54; design D5): a change
    /// of `[runs]`, `[forge]` and `[telemetry]` whose key holds no private
    /// key leaves the run on all it holds, the bound of one among it, so
    /// the second combination's run starts only once the first has ended,
    /// with the cores the first had; both land, and the run says once the
    /// file, the tables, that its settings do not bind and the key, with
    /// nothing taken up and nothing started afresh.
    #[test]
    fn a_run_keeps_its_settings_when_a_change_it_starts_afresh_for_does_not_bind() {
        use std::os::unix::fs::PermissionsExt;
        let seen = Scratch::new("instance-unbound-afresh-seen");
        let keys = Scratch::new("instance-unbound-afresh-key");
        let key = keys.path().join("app.pem");
        std::fs::write(&key, "no private key\n").unwrap();
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
        let key_line = format!("key = {:?}", key.display().to_string());
        let (s, file) = two_queued("unbound-afresh", |file| {
            check_writing(
                seen.path(),
                file,
                &[
                    "[runs]",
                    "max = 2",
                    "",
                    "[forge]",
                    "app_id = 1",
                    &key_line,
                    "",
                    "[telemetry]",
                    "send = true",
                    "collector = \"http://127.0.0.1:9\"",
                ],
                30,
            )
        });

        let out = s
            .command(s.repo.dir())
            .args(["run", "--rounds", "12"])
            .output()
            .unwrap();
        let said = text(&out);
        assert!(out.status.success(), "{said}");

        let processors = std::thread::available_parallelism().map_or(1, |n| n.get());
        assert_eq!(
            starts(seen.path()),
            [(1, processors), (1, processors)],
            "{said}"
        );
        // A command reads the file as it starts, and refuses a key that
        // does not read: the status is read under the settings the run held.
        std::fs::write(&file, "[runs]\nmax = 1\n").unwrap();
        assert_eq!(states(&s, "bolt/a"), ["landed", "landed"], "{said}");
        let refused: Vec<&str> = said
            .lines()
            .filter(|l| l.contains("which this sluice run takes up only by starting afresh"))
            .collect();
        assert_eq!(refused.len(), 1, "{said}");
        let words = format!(
            "sluice: warning: {} changed [runs], [forge], [telemetry], which this sluice run \
             takes up only by starting afresh, and its settings do not bind, so it keeps the \
             settings it holds until the file changes: ",
            file.display()
        );
        assert!(refused[0].starts_with(&words), "{said}");
        assert!(refused[0].contains(&key.display().to_string()), "{said}");
        assert!(!said.contains("is refused"), "{said}");
        assert!(!said.contains("took up"), "{said}");
    }

    /// 71: on a host that gives sluice all its cores, `[runs] cores = 2`
    /// under a bound of two gives each run one core, however many the host
    /// has; on a host that already limits sluice, `sluice run` refuses the
    /// setting, naming it, and nothing runs.
    #[test]
    fn the_keepers_cores_set_each_runs_share() {
        let repo = Repo::new("instance-cores");
        let seen = Scratch::new("instance-cores-seen");
        repo.write(".config/sluice.toml", POLICY);
        repo.write("check.sh", &check_until(seen.path(), 1));
        repo.git(&["add", "-A"]);
        repo.git(&["commit", "-q", "-m", "sluice's policy"]);
        repo.git(&["branch", "bolt/a", "main"]);
        let s = Setup {
            repo,
            sessions: Scratch::new("instance-cores-sessions"),
            config: Rc::new(host_config(2)),
            state: Rc::new(Scratch::new("instance-cores-state")),
        };
        s.submit("bolt/a", "ca");
        // Stated once the submission is in, so a host that refuses them
        // still has a combination that would run.
        std::fs::write(
            s.config.path().join("sluice/config.toml"),
            "[runs]\nmax = 2\ncores = 2\n",
        )
        .unwrap();

        let out = s
            .command(s.repo.dir())
            .args(["run", "--rounds", "16"])
            .output()
            .unwrap();
        let (given, all) = sluice::instance::host_cores();
        if given == all && all >= 2 {
            assert!(out.status.success(), "{}", text(&out));
            let starts = starts(seen.path());
            let jobs: Vec<usize> = starts.iter().map(|(_, jobs)| *jobs).collect();
            assert_eq!(jobs, [1], "{starts:?}");
            assert_eq!(states(&s, "bolt/a"), ["landed"]);
        } else {
            assert!(!out.status.success(), "{}", text(&out));
            assert!(text(&out).contains("[runs] cores"), "{}", text(&out));
            assert!(!seen.path().join("starts").exists(), "nothing runs");
        }
    }
}
