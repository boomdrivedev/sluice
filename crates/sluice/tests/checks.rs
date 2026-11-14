//! The check seam's profile with the `command` runner and the `open`
//! executor: section 4's acceptance tests in
//! `docs/plans/phase-1-integrations.md` that need no tool but `sh` and
//! git, so they are in the fast tier.

mod support;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sluice::instance::{HostCache, Instance};
use sluice::profiles::checks::{docker_host, Checks, Endpoint, EngineTls};
use sluice_seams::{
    Equip, EquipStep, Limits, Network, Outcome, Place, Progress, Refusals, RunHandle, RunKind,
    RunReport, RunRequest, Setting, Settings, Source, Tests, Through,
};
use support::{Repo, Scratch};

/// The check profile of `repo`, counting its runs in a registry of the
/// repository's own, never the host's.
fn checks(repo: &Repo) -> Checks {
    caching(repo, Vec::new())
}

/// The check profile of `repo` on a host that declares `caches`.
fn caching(repo: &Repo, caches: Vec<HostCache>) -> Checks {
    Checks::new(
        &repo.dir().join(".git"),
        PathBuf::from(env!("CARGO_BIN_EXE_sluice")),
        Instance {
            bound: 2,
            reserved: 0,
            processors: 8,
            forge: None,
            signing: None,
            peers: Default::default(),
            systems: None,
            caches,
            expire_after: sluice_core::EXPIRE_AFTER,
            telemetry: None,
        },
    )
    .with_running(repo.dir().join(".git/host-running"))
}

fn command(cmd: &str) -> Settings {
    Settings::from([
        ("runner".into(), Setting::Text("command".into())),
        ("command".into(), Setting::Text(cmd.into())),
    ])
}

fn request(repo: &Repo, name: &str, tests: Settings) -> RunRequest {
    let tmp = repo.dir().join(".git/sluice/.tmp").join(name);
    std::fs::create_dir_all(&tmp).unwrap();
    RunRequest {
        place: Place {
            name: name.into(),
            path: repo.dir().to_path_buf(),
            tmp,
            env: Default::default(),
        },
        revision: sluice_seams::Rev::new(repo.rev("HEAD")),
        required: vec![],
        since: None,
        limit: Some(Duration::from_secs(60)),
        limits: Limits::default(),
        tests,
        command: None,
        replay: true,
        system: None,
    }
}

fn finish(c: &Checks, req: &RunRequest) -> RunReport {
    ran(c, req).1
}

/// `req` started and run to its end: its handle and its report.
fn ran(c: &Checks, req: &RunRequest) -> (RunHandle, RunReport) {
    let run = support::start(c, req).unwrap();
    let until = Instant::now() + Duration::from_secs(60);
    loop {
        match c.progress(&run).unwrap() {
            Progress::Finished(r) => return (run, r),
            Progress::Lost => panic!("lost"),
            Progress::Running => {}
        }
        assert!(Instant::now() < until, "the run never finished");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// 37, 16, 84: a command's exit is its one check's outcome, observed on
/// this host by an open executor that confines nothing.
#[test]
fn a_command_passes_or_fails_by_its_exit() {
    let repo = Repo::new("checks-exit");
    let c = checks(&repo);
    let pass = finish(&c, &request(&repo, "p1", command("test -f README")));
    assert_eq!(pass.tasks.len(), 1);
    assert_eq!(pass.tasks[0].task, "test -f README");
    assert_eq!(pass.tasks[0].hash, repo.rev("HEAD"));
    assert_eq!(pass.tasks[0].outcome, Outcome::Pass);
    assert!(matches!(pass.tasks[0].source, Source::Ran { .. }));
    assert_eq!(pass.executor.kind, "open");
    assert!(!pass.executor.confined);
    assert_eq!(
        pass.executor.processors, None,
        "a command is given no share it holds"
    );
    assert!(pass.missing.is_empty() && !pass.timed_out);

    let fail = finish(&c, &request(&repo, "p2", command("exit 3")));
    assert_eq!(fail.tasks[0].outcome, Outcome::Fail);

    // Proposed 119: the command's own time, and no attempts or estimate.
    let slow = finish(&c, &request(&repo, "p3", command("sleep 0.3")));
    let took = slow.tasks[0].took_ms.expect("a command's time");
    assert!((250..10_000).contains(&took), "{took}");
    assert_eq!(slow.tasks[0].attempts, None);
    assert_eq!(slow.detail, None);
}

/// 60: a run whose folder is gone, its place released or the folder
/// removed, has no outcome: its progress reads as lost, so the round runs
/// its checks again, and never fails for it.
#[test]
fn a_run_whose_folder_is_gone_reads_as_lost() {
    let repo = Repo::new("checks-gone");
    let c = checks(&repo);
    let (run, _) = ran(&c, &request(&repo, "p1", command("true")));
    std::fs::remove_dir_all(c.runs().dir(&run.0)).unwrap();
    assert!(matches!(c.progress(&run).unwrap(), Progress::Lost));
    assert!(c.tasks(&run).unwrap().is_empty());
}

/// 66: a run over its time limit is ended and reported timed out, with no
/// outcome observed.
#[test]
fn a_command_over_its_limit_times_out() {
    let repo = Repo::new("checks-limit");
    let c = checks(&repo);
    let mut req = request(&repo, "p1", command("sleep 300"));
    req.limit = Some(Duration::from_secs(1));
    let r = finish(&c, &req);
    assert!(r.timed_out);
    assert!(r.tasks.is_empty());
}

/// Run in a child of this test binary, with sluice's own environment
/// naming another place and tool.
#[test]
fn env_probe() {
    let Ok(out) = std::env::var("SLUICE_TEST_ENV_OUT") else {
        return;
    };
    let repo = Repo::new("checks-env");
    let c = checks(&repo);
    let req = request(&repo, "p1", command("env > \"$SLUICE_TMP/env\""));
    assert!(finish(&c, &req).tasks[0].outcome == Outcome::Pass);
    let seen = std::fs::read_to_string(req.place.tmp.join("env")).unwrap();
    std::fs::write(
        &out,
        format!(
            "{}\n--\n{}\n--\n{}",
            repo.dir().display(),
            req.place.tmp.display(),
            seen
        ),
    )
    .unwrap();
}

/// 35: the command sees its place and scratch directory, its share of the
/// cores, and nothing of sluice's own that names another place or
/// tool.
#[test]
fn a_command_sees_its_place_and_nothing_of_sluices() {
    let out = support::Scratch::new("checks-env-out");
    let file = out.path().join("seen");
    let status = support::isolated(&mut std::process::Command::new(
        std::env::current_exe().unwrap(),
    ))
    .args(["--exact", "env_probe", "--nocapture"])
    .env("SLUICE_TEST_ENV_OUT", &file)
    .env("MOON_FORCE", "1")
    .env("GIT_DIR", "/elsewhere/.git")
    .env("DEVENV_ROOT", "/elsewhere")
    .status()
    .unwrap();
    assert!(status.success());
    let text = std::fs::read_to_string(&file).unwrap();
    let parts: Vec<&str> = text.splitn(3, "\n--\n").collect();
    let (place, tmp, seen) = (parts[0], parts[1], parts[2]);
    assert!(
        seen.contains(&format!("SLUICE_CHECKOUT={place}\n")),
        "{seen}"
    );
    assert!(seen.contains(&format!("SLUICE_TMP={tmp}\n")), "{seen}");
    assert!(seen.contains("SLUICE_JOBS=4\n"), "{seen}");
    for leak in ["MOON_FORCE", "GIT_DIR", "DEVENV_ROOT"] {
        assert!(!seen.contains(leak), "{leak} leaked: {seen}");
    }
}

/// Run in a child of this test binary, on a host that declares a cache's
/// variables, with sluice's own environment holding one it does not.
#[test]
fn hosts_cache_env_probe() {
    let Ok(out) = std::env::var("SLUICE_TEST_CACHE_ENV_OUT") else {
        return;
    };
    let repo = Repo::new("checks-cache-env");
    let c = caching(
        &repo,
        vec![HostCache {
            dir: "/caches/sccache".into(),
            socket: None,
            env: [("SCCACHE_DIR".to_string(), "/caches/sccache".to_string())].into(),
        }],
    );
    let req = request(&repo, "p1", command("env > \"$SLUICE_TMP/env\""));
    assert!(finish(&c, &req).tasks[0].outcome == Outcome::Pass);
    std::fs::copy(req.place.tmp.join("env"), &out).unwrap();
}

/// 35: every run is given the variables the host's config declares for
/// its caches, under the open executor too, and nothing of sluice's own
/// environment that the config does not declare.
#[test]
fn every_run_gets_the_hosts_cache_environment() {
    let out = support::Scratch::new("checks-cache-env-out");
    let file = out.path().join("seen");
    let status = support::isolated(&mut std::process::Command::new(
        std::env::current_exe().unwrap(),
    ))
    .args(["--exact", "hosts_cache_env_probe", "--nocapture"])
    .env("SLUICE_TEST_CACHE_ENV_OUT", &file)
    .env("RUSTC_WRAPPER", "sccache")
    .status()
    .unwrap();
    assert!(status.success());
    let seen = std::fs::read_to_string(&file).unwrap();
    assert!(
        seen.lines().any(|l| l == "SCCACHE_DIR=/caches/sccache"),
        "{seen}"
    );
    assert!(!seen.contains("RUSTC_WRAPPER"), "{seen}");
}

/// 35: a command `[tests] cold` lists is given none of the host's
/// declared variables; a command it does not list is given them.
#[test]
fn a_cold_command_gets_no_cache() {
    let repo = Repo::new("checks-cold");
    let c = caching(
        &repo,
        vec![HostCache {
            dir: "/caches/sccache".into(),
            socket: None,
            env: [("SCCACHE_DIR".to_string(), "/caches/sccache".to_string())].into(),
        }],
    );
    let probe = "env > \"$SLUICE_TMP/env\"";
    let seen = |name: &str, cold: &str| {
        let tests = with(
            command(probe),
            "cold",
            Setting::List(vec![cold.to_string()]),
        );
        let req = request(&repo, name, tests);
        assert_eq!(finish(&c, &req).tasks[0].outcome, Outcome::Pass);
        std::fs::read_to_string(req.place.tmp.join("env"))
            .unwrap()
            .lines()
            .any(|l| l == "SCCACHE_DIR=/caches/sccache")
    };
    assert!(!seen("p1", probe), "a cold command got the cache");
    assert!(seen("p2", "tests/bench"), "a warm command got no cache");
}

/// 35: a declared cache that is the run's checkout, holds it or lies
/// inside it refuses the run before anything of it runs, naming the cache.
#[test]
fn a_cache_holding_the_checkout_is_refused() {
    let repo = Repo::new("checks-cache-checkout");
    let parent = repo.dir().parent().unwrap().to_path_buf();
    for (name, dir, how) in [
        ("p1", parent, "holds"),
        ("p2", repo.dir().to_path_buf(), "is"),
        ("p3", repo.dir().join(".cache/sccache"), "lies inside"),
    ] {
        let c = caching(
            &repo,
            vec![HostCache {
                dir: dir.clone(),
                socket: None,
                env: Default::default(),
            }],
        );
        let err = format!(
            "{:#}",
            support::start(&c, &request(&repo, name, command("touch ran"))).unwrap_err()
        );
        assert!(
            err.contains(&format!("dir {} {how} this run's checkout", dir.display())),
            "{err}"
        );
        assert!(err.contains("outside every checkout"), "{err}");
        assert!(!c.runs().dir(name).join("request.json").exists());
    }
    assert!(!repo.dir().join("ran").exists());
}

/// `[tests] toolchain` runs before the run and what it prints is sourced
/// before the command; anything else the script prints never reaches the
/// command's output.
#[test]
fn a_command_runs_with_the_repositorys_toolchain() {
    let repo = Repo::new("checks-toolchain");
    repo.write(
        "tools",
        // bash 4's case fall-through, as real toolchains print: sourced by a
        // bash 3 it stops the script before the export.
        "#!/bin/sh\necho 'echo setting up'\n\
         echo 'case x in x) true ;& y) true ;; esac'\n\
         echo 'export ENTERED=yes'\n",
    );
    repo.git(&["add", "tools"]);
    repo.git(&["commit", "-q", "-m", "toolchain"]);
    std::fs::set_permissions(
        repo.dir().join("tools"),
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    let c = checks(&repo);
    let mut tests = command("test \"$ENTERED\" = yes");
    tests.insert(
        "toolchain".into(),
        Setting::List(vec![repo.dir().join("tools").to_string_lossy().into()]),
    );
    let r = finish(&c, &request(&repo, "p1", tests));
    assert_eq!(r.tasks[0].outcome, Outcome::Pass);
    // A toolchain that fails refuses the run.
    let mut tests = command("true");
    tests.insert("toolchain".into(), Setting::List(vec!["false".into()]));
    assert!(support::start(&c, &request(&repo, "p2", tests)).is_err());
}

/// 83, 84: a limit the open executor cannot hold refuses the run before
/// anything runs, naming the limit.
#[test]
fn the_open_executor_refuses_what_it_cannot_hold() {
    let repo = Repo::new("checks-refuse");
    let c = checks(&repo);
    let refused = |limits: Limits, word: &str| {
        let mut req = request(&repo, "p1", command("touch ran"));
        req.limits = limits;
        let err = support::start(&c, &req).unwrap_err();
        assert!(format!("{err:#}").contains(word), "{err:#}");
        assert!(!repo.dir().join("ran").exists());
        assert!(!c.runs().dir("p1").exists());
    };
    refused(
        Limits {
            memory: Some(1 << 30),
            ..Limits::default()
        },
        "memory",
    );
    for network in [Network::Denied, Network::Localhost] {
        refused(
            Limits {
                network: Some(network),
                ..Limits::default()
            },
            "network",
        );
    }
    refused(
        Limits {
            executor: Some("container".into()),
            ..Limits::default()
        },
        "container",
    );
    // On macOS the sandbox cannot hold memory; where it is not built yet,
    // it refuses the run for that.
    refused(
        Limits {
            executor: Some("sandbox".into()),
            memory: Some(1 << 30),
            ..Limits::default()
        },
        if cfg!(target_os = "macos") {
            "memory"
        } else {
            "not built"
        },
    );
    // A network the policy allows needs no holding.
    let mut req = request(&repo, "p2", command("true"));
    req.limits.network = Some(Network::Allowed);
    assert_eq!(finish(&c, &req).tasks[0].outcome, Outcome::Pass);
}

/// 71: the bound is the instance's.
#[test]
fn the_bound_is_the_instances() {
    let repo = Repo::new("checks-bound");
    assert_eq!(checks(&repo).bound().unwrap(), 2);
}

/// 71, S60 (rule core.49; record 0087): the bound is the host's, shared
/// by every repository on it. A second repository's hold while the host's
/// one run goes is refused before anything of its run runs, and holds
/// once that run ends.
#[test]
fn a_second_repositorys_start_waits_for_the_hosts_bound() {
    let host = Scratch::new("checks-host-running");
    let (a, b) = (Repo::new("checks-host-a"), Repo::new("checks-host-b"));
    let at = |repo: &Repo| {
        Checks::new(
            &repo.dir().join(".git"),
            PathBuf::from(env!("CARGO_BIN_EXE_sluice")),
            Instance {
                bound: 1,
                reserved: 0,
                processors: 8,
                forge: None,
                signing: None,
                peers: Default::default(),
                systems: None,
                caches: Vec::new(),
                expire_after: sluice_core::EXPIRE_AFTER,
                telemetry: None,
            },
        )
        .with_running(host.path().to_path_buf())
    };
    let (first, second) = (at(&a), at(&b));
    let go = a.dir().join(".git/go");
    let holds = format!("while [ ! -e '{}' ]; do sleep 0.05; done", go.display());
    let going = support::start(&first, &request(&a, "sluice/try/main/1", command(&holds))).unwrap();

    let req = request(&b, "sluice/try/main/1", command("touch ran"));
    assert!(
        second.hold(RunKind::Queue).unwrap().is_none(),
        "the host's one run is the first repository's"
    );
    let dir = second.runs().dir("sluice/try/main/1");
    assert!(!dir.join("request.json").exists(), "nothing of it ran");
    assert!(!b.dir().join("ran").exists());
    assert_eq!(second.running().unwrap().queue, 1, "the other's run counts");

    std::fs::write(&go, "").unwrap();
    let until = Instant::now() + Duration::from_secs(60);
    while matches!(first.progress(&going).unwrap(), Progress::Running) {
        assert!(Instant::now() < until, "the first run never ended");
        std::thread::sleep(Duration::from_millis(50));
    }
    let part = second.hold(RunKind::Queue).unwrap().expect("the run ended");
    let run = second.start(&req, part).unwrap();
    let until = Instant::now() + Duration::from_secs(60);
    let report = loop {
        match second.progress(&run).unwrap() {
            Progress::Finished(r) => break r,
            Progress::Lost => panic!("lost"),
            Progress::Running => {}
        }
        assert!(Instant::now() < until, "the run never finished");
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(report.tasks[0].outcome, Outcome::Pass);
    assert!(b.dir().join("ran").exists());
}

/// A policy that names no runner, or one sluice lacks, refuses the run.
#[test]
fn an_unknown_runner_is_refused() {
    let repo = Repo::new("checks-runner");
    let c = checks(&repo);
    assert!(support::start(&c, &request(&repo, "p1", Settings::new())).is_err());
    let tests = Settings::from([("runner".into(), Setting::Text("make".into()))]);
    assert!(support::start(&c, &request(&repo, "p2", tests)).is_err());
}

/// 83, 84: on macOS the sandbox holds a run to writing its place and
/// scratch directory and, with `network = false`, to no connection; the
/// record says sandbox, not confined, network held.
#[cfg(target_os = "macos")]
#[test]
fn the_sandbox_holds_writes_and_the_network() {
    let repo = Repo::new("checks-sandbox");
    let outside = repo.dir().with_extension("outside");
    std::fs::create_dir_all(&outside).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let c = checks(&repo);
    let sandboxed = |name: &str, cmd: &str, network: Option<Network>| {
        let mut req = request(&repo, name, command(cmd));
        req.limits = Limits {
            executor: Some("sandbox".into()),
            network,
            memory: None,
            docker: false,
        };
        finish(&c, &req)
    };
    let inside = sandboxed(
        "p1",
        "touch \"$SLUICE_CHECKOUT/in\" \"$SLUICE_TMP/in\" \"$TMPDIR/t\"",
        Some(Network::Denied),
    );
    assert_eq!(inside.tasks[0].outcome, Outcome::Pass);
    assert_eq!(inside.executor.kind, "sandbox");
    assert!(!inside.executor.confined);
    assert_eq!(inside.executor.network, Some(Network::Denied));

    let escape = format!("touch {}/escaped", outside.display());
    assert_eq!(
        sandboxed("p2", &escape, None).tasks[0].outcome,
        Outcome::Fail
    );
    assert!(!outside.join("escaped").exists());

    // A command in a checkout that lists proto's tools may stamp them as
    // used where they are installed, as a moon run may; nothing else
    // outside is opened to it.
    let stamp = format!("touch {}/.last-used", outside.display());
    assert_eq!(
        sandboxed("p7", &stamp, None).tasks[0].outcome,
        Outcome::Fail
    );
    std::fs::write(repo.dir().join(".prototools"), "").unwrap();
    assert_eq!(
        sandboxed("p8", &stamp, None).tasks[0].outcome,
        Outcome::Pass
    );
    assert_eq!(
        sandboxed("p9", &escape, None).tasks[0].outcome,
        Outcome::Fail
    );

    let connect = format!("/usr/bin/nc -z -w 2 127.0.0.1 {port}");
    assert_eq!(
        sandboxed("p3", &connect, Some(Network::Denied)).tasks[0].outcome,
        Outcome::Fail
    );
    let open = sandboxed("p4", &connect, None);
    assert_eq!(open.tasks[0].outcome, Outcome::Pass);
    assert_eq!(open.executor.network, None);

    // Localhost only: a server on localhost is reached; any other address
    // is refused by the sandbox, not merely unanswered.
    let local = sandboxed("p5", &connect, Some(Network::Localhost));
    assert_eq!(local.tasks[0].outcome, Outcome::Pass);
    assert_eq!(local.executor.network, Some(Network::Localhost));
    // Passes only when the connection is refused as not permitted.
    let elsewhere = "/usr/bin/perl -MIO::Socket::INET -e \
        'IO::Socket::INET->new(PeerAddr => \"192.0.2.1:9\", Timeout => 1) and exit 2; \
         exit($! =~ /not permitted/ ? 0 : 1)'";
    assert_eq!(
        sandboxed("p6", elsewhere, Some(Network::Localhost)).tasks[0].outcome,
        Outcome::Pass
    );
    drop(listener);
}

/// 83, 138, design D1 (the check-grant spec's "A sandboxed run"): a
/// sandboxed run's report carries the grant of the profile it ran under,
/// the same as the same request's run in another place, and another than
/// the same run held to no network.
#[cfg(target_os = "macos")]
#[test]
fn a_sandbox_run_records_the_grant_of_its_profile() {
    let (one, two) = (
        Repo::new("checks-sandbox-grant-one"),
        Repo::new("checks-sandbox-grant-two"),
    );
    let run = |repo: &Repo, name: &str, network: Option<Network>| {
        let mut req = request(repo, name, command("true"));
        req.limits = Limits {
            executor: Some("sandbox".into()),
            network,
            memory: None,
            docker: false,
        };
        let report = finish(&checks(repo), &req);
        assert_eq!(report.tasks[0].outcome, Outcome::Pass);
        report.executor.grant.expect("a sandboxed run's grant")
    };
    let here = run(&one, "p1", Some(Network::Localhost));
    assert!(here.starts_with("sb1:"), "{here}");
    assert_eq!(run(&two, "p2", Some(Network::Localhost)), here);
    assert_ne!(run(&one, "p3", Some(Network::Denied)), here);
}

/// `req` held to the sandbox, with `network`.
fn sandboxed(mut req: RunRequest, network: Option<Network>) -> RunRequest {
    req.limits = Limits {
        executor: Some("sandbox".into()),
        network,
        memory: None,
        docker: false,
    };
    req
}

/// 139, record 0072 (the run-outcome spec's "What the sandbox refused"):
/// a sandboxed run that writes outside its checkout and fails answers what
/// the sandbox refused it, the write's operation and path and the program
/// that tried, from the sandbox's own report.
#[cfg(target_os = "macos")]
#[test]
fn the_sandbox_reports_what_it_refused_a_run() {
    let repo = Repo::new("checks-sandbox-refused");
    let outside = repo.dir().with_extension("outside");
    std::fs::create_dir_all(&outside).unwrap();
    let c = checks(&repo);
    let escape = format!("/usr/bin/touch {}/escaped", outside.display());
    let (run, report) = ran(&c, &sandboxed(request(&repo, "p1", command(&escape)), None));
    assert_eq!(report.tasks[0].outcome, Outcome::Fail);
    match c.refused(&run).unwrap() {
        Refusals::Refused(words) => {
            assert!(
                words.starts_with("sluice's sandbox refused the run file-write-create "),
                "{words}"
            );
            assert!(words.contains("/escaped (by touch)"), "{words}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(c.refused(&run).unwrap(), c.refused(&run).unwrap());
}

/// 38, 139, rule core.35 (the run-outcome spec's "A test that prints a
/// refusal"): a sandboxed run that writes nothing outside its checkout,
/// prints "Operation not permitted" and fails is refused nothing, and so
/// is an open run that fails.
#[cfg(target_os = "macos")]
#[test]
fn a_sandbox_run_that_writes_nothing_outside_reports_no_refusal() {
    let repo = Repo::new("checks-sandbox-no-refusal");
    let c = checks(&repo);
    let says = "echo 'touch: /x: Operation not permitted' >&2; exit 1";
    let (run, report) = ran(&c, &sandboxed(request(&repo, "p1", command(says)), None));
    assert_eq!(report.tasks[0].outcome, Outcome::Fail);
    assert_eq!(c.refused(&run).unwrap(), Refusals::None);
    let (run, report) = ran(&c, &request(&repo, "p2", command(says)));
    assert_eq!(report.tasks[0].outcome, Outcome::Fail);
    assert_eq!(c.refused(&run).unwrap(), Refusals::None);
}

/// 139, design D4 (the run-outcome spec's "Two runs at once"): two
/// sandboxed runs started together, each failing, one writing outside its
/// checkout: only that one reads a refusal, since each reads only its own
/// mark.
#[cfg(target_os = "macos")]
#[test]
fn two_sandbox_runs_at_once_report_only_their_own_refusals() {
    let repo = Repo::new("checks-sandbox-two-runs");
    let outside = repo.dir().with_extension("outside");
    std::fs::create_dir_all(&outside).unwrap();
    let c = checks(&repo);
    let escape = format!(
        "sleep 1; touch {}/escaped; sleep 1; exit 1",
        outside.display()
    );
    let stays = "sleep 1; touch \"$SLUICE_TMP/in\"; sleep 1; exit 1";
    let one = support::start(&c, &sandboxed(request(&repo, "p1", command(&escape)), None)).unwrap();
    let two = support::start(&c, &sandboxed(request(&repo, "p2", command(stays)), None)).unwrap();
    for run in [&one, &two] {
        let until = Instant::now() + Duration::from_secs(60);
        while !matches!(c.progress(run).unwrap(), Progress::Finished(_)) {
            assert!(Instant::now() < until, "the run never finished");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    assert!(
        matches!(c.refused(&one).unwrap(), Refusals::Refused(w) if w.contains("/escaped")),
        "{:?}",
        c.refused(&one)
    );
    assert_eq!(c.refused(&two).unwrap(), Refusals::None);
}

/// 139, design D5, D8 (the run-outcome spec's "The log cannot be
/// streamed"): where the program the sandbox's reports are streamed with
/// exits at once, saying why, a sandboxed run that fails answers its
/// refusals unread, holding those words.
#[cfg(target_os = "macos")]
#[test]
fn a_sandbox_run_whose_log_cannot_be_read_reports_it_unread() {
    let repo = Repo::new("checks-sandbox-log-unread");
    let log = repo.dir().with_extension("log.sh");
    std::fs::write(&log, "#!/bin/sh\necho 'no log here' >&2\nexit 1\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&log, std::fs::Permissions::from_mode(0o755)).unwrap();
    let c = checks(&repo).with_log(log);
    let (run, report) = ran(
        &c,
        &sandboxed(request(&repo, "p1", command("exit 1")), None),
    );
    assert_eq!(report.tasks[0].outcome, Outcome::Fail);
    match c.refused(&run).unwrap() {
        Refusals::Unread(words) => assert!(words.contains("no log here"), "{words}"),
        other => panic!("{other:?}"),
    }
}

/// Design D5, ADR 0016; the host-requirements spec's "An engine at a TCP
/// address": a sandboxed run with `network = "denied"` that may reach the
/// container engine reaches it at its TCP address, as
/// `DOCKER_HOST=tcp://127.0.0.1:<port>` names it, and the same run without
/// `docker` is refused the connection. An engine named by a host name is
/// opened on its port alone, with the resolver's socket.
#[cfg(target_os = "macos")]
#[test]
fn the_sandbox_opens_the_engines_tcp_address() {
    let repo = Repo::new("checks-sandbox-engine-tcp");
    let port = support::tcp_engine("HTTP/1.0 200 OK");
    let c = checks(&repo).with_engine_endpoints(vec![local(port, None)]);
    let connect = format!("/usr/bin/nc -z -w 2 127.0.0.1 {port}");
    let run = |c: &Checks, name: &str, cmd: &str, docker| {
        let mut req = request(&repo, name, command(cmd));
        req.limits = Limits {
            executor: Some("sandbox".into()),
            network: Some(Network::Denied),
            memory: None,
            docker,
        };
        finish(c, &req).tasks[0].outcome
    };
    assert_eq!(run(&c, "p1", &connect, true), Outcome::Pass);
    assert_eq!(run(&c, "p2", &connect, false), Outcome::Fail);

    let named = checks(&repo).with_engine_endpoints(vec![Endpoint::Tcp {
        host: "build-engine".into(),
        port: 2376,
        tls: None,
    }]);
    assert_eq!(run(&named, "p3", "true", true), Outcome::Pass);
    let profile = std::fs::read_to_string(named.runs().dir("p3").join("sandbox.sb")).unwrap();
    assert!(
        profile.contains("(allow network-outbound (remote tcp \"*:2376\"))"),
        "{profile}"
    );
    assert!(
        profile.contains("/private/var/run/mDNSResponder"),
        "{profile}"
    );
}

/// C.3: a sandboxed command may open a pseudo-terminal, as a test that
/// starts a terminal multiplexer does.
#[cfg(target_os = "macos")]
#[test]
fn the_sandbox_lets_a_command_open_a_pseudo_terminal() {
    let repo = Repo::new("checks-sandbox-pty");
    let c = checks(&repo);
    let mut req = request(
        &repo,
        "p1",
        command("/usr/bin/script -q /dev/null /usr/bin/true"),
    );
    req.limits = Limits {
        executor: Some("sandbox".into()),
        network: Some(Network::Denied),
        memory: None,
        docker: false,
    };
    assert_eq!(finish(&c, &req).tasks[0].outcome, Outcome::Pass);
}

/// 35, C.2: a sandboxed run may write the host's declared cache directory
/// and reach its declared socket with no network, and write nothing
/// beside them; a declared socket nobody listens on changes nothing, and
/// sluice makes nothing of a cache.
#[cfg(target_os = "macos")]
#[test]
fn the_sandbox_opens_the_hosts_caches() {
    let repo = Repo::new("checks-sandbox-caches");
    // A short name, so the socket's path fits a Unix socket's address.
    let host = Scratch::new("sk");
    let (dir, beside) = (host.path().join("c"), host.path().join("b"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir_all(&beside).unwrap();
    let socket = host.path().join("s");
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    let cache = |dir: &std::path::Path, socket: &std::path::Path| HostCache {
        dir: dir.to_path_buf(),
        socket: Some(socket.to_path_buf()),
        env: Default::default(),
    };
    let sandboxed = |c: &Checks, name: &str, cmd: &str, network: Option<Network>| {
        let mut req = request(&repo, name, command(cmd));
        req.limits = Limits {
            executor: Some("sandbox".into()),
            network,
            memory: None,
            docker: false,
        };
        finish(c, &req).tasks[0].outcome
    };
    let c = caching(&repo, vec![cache(&dir, &socket)]);

    let write = |d: &std::path::Path| format!("touch {}/built", d.display());
    assert_eq!(
        sandboxed(&c, "p1", &write(&dir), Some(Network::Denied)),
        Outcome::Pass
    );
    assert!(dir.join("built").exists());
    assert_eq!(sandboxed(&c, "p2", &write(&beside), None), Outcome::Fail);
    assert!(!beside.join("built").exists());

    let connect = format!(
        "/usr/bin/perl -MIO::Socket::UNIX -e 'IO::Socket::UNIX->new(Peer => \"{}\") or exit 1'",
        socket.display()
    );
    assert_eq!(
        sandboxed(&c, "p3", &connect, Some(Network::Denied)),
        Outcome::Pass
    );
    // The declaration opens it: with no cache, no network reaches nothing.
    assert_eq!(
        sandboxed(&checks(&repo), "p4", &connect, Some(Network::Denied)),
        Outcome::Fail
    );
    drop(listener);

    // A server that is down: the run is as one with no cache, and nothing
    // of the cache is made or left.
    let (gone, quiet) = (host.path().join("none"), host.path().join("q"));
    let down = caching(&repo, vec![cache(&gone, &quiet)]);
    assert_eq!(sandboxed(&down, "p5", "true", None), Outcome::Pass);
    assert_eq!(sandboxed(&checks(&repo), "p6", "true", None), Outcome::Pass);
    assert!(!gone.exists() && !quiet.exists());
}

/// The limits of a run that may reach the container engine.
fn docker() -> Limits {
    Limits {
        docker: true,
        ..Limits::default()
    }
}

/// 84, record 0074 (design D2): an engine that answers `200` to `GET
/// /_ping` at a socket the run would reach meets a run that asks for one;
/// a path with no socket beside it is passed over.
#[test]
fn an_engine_that_answers_meets_a_run_that_asks_for_one() {
    let repo = Repo::new("checks-engine");
    let dir = Scratch::short();
    let socket = support::engine(dir.path(), "HTTP/1.0 200 OK");
    let c = checks(&repo).with_engine_sockets(vec![dir.path().join("none.sock"), socket]);
    assert_eq!(c.unmet(&docker()).unwrap(), None);
}

/// 84, record 0074 (design D2): a socket that answers anything but `200`,
/// one nothing listens on, and none at all each leave a run that asks for
/// an engine unmet, saying where the look found none and what it found.
#[test]
fn a_host_whose_engine_does_not_answer_does_not_meet_it() {
    let repo = Repo::new("checks-no-engine");

    let dir = Scratch::short();
    let socket = support::engine(dir.path(), "HTTP/1.0 500 Internal Server Error");
    let c = checks(&repo).with_engine_sockets(vec![socket.clone()]);
    let why = c.unmet(&docker()).unwrap().expect("unmet");
    assert!(why.contains(&socket.display().to_string()), "{why}");
    assert!(why.contains("500"), "{why}");

    let dir = Scratch::short();
    let (socket, why) = support::at_a_closed_socket(
        dir.path(),
        |socket| {
            let c = checks(&repo).with_engine_sockets(vec![socket.to_path_buf()]);
            c.unmet(&docker()).unwrap().expect("unmet")
        },
        |_, why| why.contains("refused"),
    );
    assert!(socket.exists(), "the socket nothing listens on stays");
    assert!(why.contains(&socket.display().to_string()), "{why}");
    assert!(why.contains("refused"), "{why}");

    let dir = Scratch::short();
    let socket = dir.path().join("e.sock");
    let c = checks(&repo).with_engine_sockets(vec![socket.clone()]);
    let why = c.unmet(&docker()).unwrap().expect("unmet");
    assert_eq!(
        why,
        format!(
            "no container engine answers: no socket at {}",
            socket.display()
        )
    );
}

/// 84: a run that asks for no engine is met, with no socket looked at.
#[test]
fn a_run_that_asks_for_no_engine_is_met_with_none() {
    let repo = Repo::new("checks-engine-unasked");
    let dir = Scratch::short();
    let c = checks(&repo).with_engine_sockets(vec![dir.path().join("e.sock")]);
    assert_eq!(c.unmet(&Limits::default()).unwrap(), None);
}

/// 84, record 0097 (design D8): a link of the test's own in place of the
/// host's `/run/current-system`, in `dir`, pointed at `<dir>/<to>` where
/// `to` is given.
fn system_link(dir: &Path, to: Option<&str>) -> PathBuf {
    let link = dir.join("current-system");
    if let Some(to) = to {
        std::os::unix::fs::symlink(dir.join(to), &link).unwrap();
    }
    link
}

/// A command that does `then` to the system link at `link`, and fails.
fn and_fail(then: &str, link: &Path) -> Settings {
    command(&format!("{then} '{}'; exit 1", link.display()))
}

/// 84, 114, record 0097 (design D1, D4, D5): a run during which the
/// active system's link moved from one store path to another answers that
/// the host's system was switched, naming both.
#[test]
fn a_run_whose_system_was_switched_says_from_what_to_what() {
    let repo = Repo::new("checks-switched");
    let dir = Scratch::short();
    let link = system_link(dir.path(), Some("aaa"));
    let c = checks(&repo).with_system_link(link.clone());
    let to = dir.path().join("bbb");
    let then = format!("ln -sfn '{}'", to.display());
    let (run, r) = ran(&c, &request(&repo, "p1", and_fail(&then, &link)));
    assert_eq!(r.tasks[0].outcome, Outcome::Fail);
    assert_eq!(
        c.switched(&run).unwrap(),
        Some(format!(
            "the host's system was switched during the run, from {} to {}",
            dir.path().join("aaa").display(),
            to.display()
        ))
    );
}

/// 84, record 0097 (design D4): a run during which the link stood answers
/// no switch.
#[test]
fn a_run_whose_system_stood_says_nothing() {
    let repo = Repo::new("checks-system-stood");
    let dir = Scratch::short();
    let link = system_link(dir.path(), Some("aaa"));
    let c = checks(&repo).with_system_link(link.clone());
    let (run, r) = ran(&c, &request(&repo, "p1", and_fail("test -L", &link)));
    assert_eq!(r.tasks[0].outcome, Outcome::Fail);
    assert_eq!(c.switched(&run).unwrap(), None);
}

/// 84, record 0097 (design D4), the host-requirements spec's "No Nix
/// system": a host with no link as the run starts is one Nix doesn't
/// manage, and a link made during the run reads no switch.
#[test]
fn a_host_with_no_system_link_reads_nothing() {
    let repo = Repo::new("checks-no-system");
    let dir = Scratch::short();
    let link = system_link(dir.path(), None);
    let c = checks(&repo).with_system_link(link.clone());
    let then = format!("ln -s '{}'", dir.path().join("bbb").display());
    let (run, r) = ran(&c, &request(&repo, "p1", and_fail(&then, &link)));
    assert_eq!(r.tasks[0].outcome, Outcome::Fail);
    assert!(link.is_symlink(), "the command made the link");
    assert_eq!(c.switched(&run).unwrap(), None);
}

/// 84, record 0097 (design D4): a link gone after the run reads no
/// switch, since a switch replaces the link and never removes it.
#[test]
fn a_link_gone_after_the_run_reads_no_switch() {
    let repo = Repo::new("checks-system-gone");
    let dir = Scratch::short();
    let link = system_link(dir.path(), Some("aaa"));
    let c = checks(&repo).with_system_link(link.clone());
    let (run, r) = ran(&c, &request(&repo, "p1", and_fail("rm", &link)));
    assert_eq!(r.tasks[0].outcome, Outcome::Fail);
    assert!(!link.is_symlink(), "the command removed the link");
    assert_eq!(c.switched(&run).unwrap(), None);
}

/// A TCP endpoint on this host's loopback at `port`, plain or over `tls`.
fn local(port: u16, tls: Option<EngineTls>) -> Endpoint {
    Endpoint::Tcp {
        host: "127.0.0.1".into(),
        port,
        tls,
    }
}

/// Design D5; the host-requirements spec's "An engine at a TCP address"
/// and "Nothing answers at the TCP address": an engine answering `200` at
/// a TCP address meets a run that asks for one; an address nothing
/// listens on, and one answering `500`, leave it unmet, naming the address
/// and what it did.
#[test]
fn an_engine_at_a_tcp_address_is_looked_for_there() {
    let repo = Repo::new("checks-engine-tcp");
    let port = support::tcp_engine("HTTP/1.0 200 OK");
    let c = checks(&repo).with_engine_endpoints(vec![local(port, None)]);
    assert_eq!(c.unmet(&docker()).unwrap(), None);

    let refusal = |closed: u16| {
        format!("no container engine answers: tcp://127.0.0.1:{closed} refused the connection")
    };
    let (closed, why) = support::at_a_closed_port(
        |closed| {
            let c = checks(&repo).with_engine_endpoints(vec![local(closed, None)]);
            c.unmet(&docker()).unwrap()
        },
        |closed, why| why.as_deref() == Some(refusal(closed).as_str()),
    );
    assert_eq!(why.as_deref(), Some(refusal(closed).as_str()));

    let failing = support::tcp_engine("HTTP/1.0 500 Internal Server Error");
    let c = checks(&repo).with_engine_endpoints(vec![local(failing, None)]);
    assert_eq!(
        c.unmet(&docker()).unwrap().as_deref(),
        Some(
            format!("no container engine answers: tcp://127.0.0.1:{failing} answered 500 Internal Server Error").as_str()
        )
    );
}

/// Design D5; the host-requirements spec's "An engine over TLS": an engine
/// that takes only a client certificate its own CA signed is found with
/// the certificate and key `DOCKER_CERT_PATH` holds, verified against its
/// `ca.pem`; without them, or verified against another CA, it is not.
#[test]
fn an_engine_over_tls_is_found_with_the_clients_certificate() {
    let repo = Repo::new("checks-engine-tls");
    let certs = Scratch::new("checks-engine-tls-certs");
    let port = support::tls_engine(certs.path());
    let tls = |verify, certs: &std::path::Path| {
        Some(EngineTls {
            verify,
            certs: certs.to_path_buf(),
        })
    };
    let c = checks(&repo).with_engine_endpoints(vec![local(port, tls(true, certs.path()))]);
    assert_eq!(c.unmet(&docker()).unwrap(), None);

    // No client certificate: the engine refuses the connection's TLS.
    let bare = Scratch::new("checks-engine-tls-bare");
    std::fs::copy(certs.path().join("ca.pem"), bare.path().join("ca.pem")).unwrap();
    let c = checks(&repo).with_engine_endpoints(vec![local(port, tls(true, bare.path()))]);
    let why = c.unmet(&docker()).unwrap().expect("unmet");
    assert!(
        why.starts_with(&format!(
            "no container engine answers: tcp://127.0.0.1:{port} "
        )),
        "{why}"
    );

    // Another CA: the engine is not the one its client trusts.
    let other = Scratch::new("checks-engine-tls-other");
    support::tls_engine(other.path());
    for f in ["cert.pem", "key.pem"] {
        std::fs::copy(certs.path().join(f), other.path().join(f)).unwrap();
    }
    let c = checks(&repo).with_engine_endpoints(vec![local(port, tls(true, other.path()))]);
    let why = c.unmet(&docker()).unwrap().expect("unmet");
    assert!(why.contains("invalid peer certificate"), "{why}");
}

/// Design D5: `DOCKER_HOST` read as the engine's client reads it.
#[test]
fn docker_host_names_a_unix_socket_or_a_tcp_address() {
    let tls = Some(EngineTls {
        verify: true,
        certs: "/certs".into(),
    });
    assert_eq!(
        docker_host("unix:///var/run/docker.sock", None),
        Some(Endpoint::Unix("/var/run/docker.sock".into()))
    );
    assert_eq!(
        docker_host("tcp://127.0.0.1:2375", None),
        Some(local(2375, None))
    );
    assert_eq!(
        docker_host("tcp://127.0.0.1", None),
        Some(local(2375, None))
    );
    assert_eq!(
        docker_host("tcp://127.0.0.1", tls.clone()),
        Some(local(2376, tls.clone()))
    );
    assert_eq!(
        docker_host("tcp://[::1]:2375/", None),
        Some(Endpoint::Tcp {
            host: "::1".into(),
            port: 2375,
            tls: None
        })
    );
    assert_eq!(
        docker_host("tcp://build-engine:2376", tls.clone())
            .unwrap()
            .to_string(),
        "tcp://build-engine:2376"
    );
    assert_eq!(docker_host("ssh://me@build-engine", None), None);
    assert_eq!(docker_host("npipe:////./pipe/docker_engine", None), None);
}

/// 84, 87 (design D2): a limit the executor cannot hold leaves a run
/// unmet in the words its start refuses it with, the open executor's and
/// the macOS sandbox's, before any socket is looked at; limits the
/// executor holds are met, and a run that asks for no engine has none
/// looked for.
#[test]
fn a_limit_the_executor_cannot_hold_leaves_a_run_unmet_before_any_socket() {
    let repo = Repo::new("checks-limits-unmet");
    let dir = Scratch::short();
    let socket = dir.path().join("e.sock");
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let c = checks(&repo).with_engine_sockets(vec![socket]);
    let gig = Some(1 << 30);
    let open = Limits {
        memory: gig,
        ..docker()
    };
    assert_eq!(
        c.unmet(&open).unwrap().as_deref(),
        Some("[executor] memory = 1073741824 bytes: the open executor cannot hold a memory limit")
    );
    let offline = Limits {
        network: Some(Network::Denied),
        ..Limits::default()
    };
    assert_eq!(
        c.unmet(&offline).unwrap().as_deref(),
        Some("[executor] network = Denied: the open executor cannot hold a run's network")
    );
    if cfg!(target_os = "macos") {
        let sandbox = |memory, network| Limits {
            executor: Some("sandbox".into()),
            memory,
            network,
            docker: false,
        };
        assert_eq!(
            c.unmet(&sandbox(gig, None)).unwrap().as_deref(),
            Some(
                "[executor] memory = 1073741824 bytes: the sandbox executor on macOS cannot \
                 hold a memory limit"
            )
        );
        assert_eq!(
            c.unmet(&sandbox(None, Some(Network::Denied))).unwrap(),
            None
        );
    }
    assert_eq!(c.unmet(&Limits::default()).unwrap(), None);
    assert!(
        matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock),
        "no socket was looked at"
    );
}

/// `tests` with `key` set to `value`.
fn with(mut tests: Settings, key: &str, value: Setting) -> Settings {
    tests.insert(key.into(), value);
    tests
}

/// The checks a run recorded as failed.
fn fails(r: &RunReport) -> Vec<String> {
    r.tasks
        .iter()
        .filter(|t| t.outcome == Outcome::Fail)
        .map(|t| t.task.clone())
        .collect()
}

/// Run in a child of this test binary whose `PATH` holds git and no
/// `bash`, so a run with a toolchain cannot start.
#[test]
fn cannot_start_probe() {
    let Ok(out) = std::env::var("SLUICE_TEST_START_OUT") else {
        return;
    };
    let repo = Repo::new("checks-cannot-start");
    let c = checks(&repo);
    let tests = with(
        command("true"),
        "toolchain",
        Setting::List(vec!["/usr/bin/true".into()]),
    );
    let r = finish(&c, &request(&repo, "p1", tests));
    let not_run: Vec<String> = r
        .not_run
        .iter()
        .map(|n| format!("{}: {}", n.task, n.reason))
        .collect();
    std::fs::write(
        &out,
        format!("{}\n--\n{}", fails(&r).join(","), not_run.join("\n")),
    )
    .unwrap();
}

/// could-not-run-1 114: a check whose program could not be started did
/// not judge it, so no fail is recorded under its key.
#[test]
fn a_check_that_cannot_start_records_no_fail() {
    let out = support::Scratch::new("checks-start-out");
    let file = out.path().join("fails");
    let bin = out.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let git = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .map(|d| d.join("git"))
        .find(|g| g.is_file())
        .expect("git on the path");
    std::os::unix::fs::symlink(git, bin.join("git")).unwrap();
    let status = support::isolated(&mut std::process::Command::new(
        std::env::current_exe().unwrap(),
    ))
    .args(["--exact", "cannot_start_probe", "--nocapture"])
    .env("SLUICE_TEST_START_OUT", &file)
    .env("PATH", &bin)
    .status()
    .unwrap();
    assert!(status.success());
    let text = std::fs::read_to_string(&file).unwrap();
    let (fails, not_run) = text.split_once("\n--\n").unwrap();
    assert!(fails.is_empty(), "recorded as failed: {fails}");
    assert!(not_run.starts_with("true: "), "{not_run}");
    assert!(
        not_run.contains("bash"),
        "names what could not start: {not_run}"
    );
}

/// could-not-run-1 114: an exit the policy's `not_run` names says the
/// check could not run here: no fail under its key. Any other exit is the
/// check's own.
#[test]
fn an_exit_the_policy_names_records_no_fail() {
    let repo = Repo::new("checks-not-run");
    let c = checks(&repo);
    let not_run = Setting::List(vec!["77".into()]);
    let r = finish(
        &c,
        &request(
            &repo,
            "p1",
            with(command("exit 77"), "not_run", not_run.clone()),
        ),
    );
    assert!(fails(&r).is_empty(), "recorded as failed: {:?}", r.tasks);
    assert_eq!(r.not_run.len(), 1);
    assert_eq!(r.not_run[0].task, "exit 77");
    assert!(r.not_run[0].reason.contains("exited 77"), "{:?}", r.not_run);
    assert!(r.missing.is_empty());
    let r = finish(
        &c,
        &request(&repo, "p2", with(command("exit 1"), "not_run", not_run)),
    );
    assert_eq!(fails(&r), ["exit 1"]);
    assert!(r.not_run.is_empty());
}

/// 84 (record 0066 as amended 2026-10-30): sluice runs no readiness
/// command before a run; a policy that names `ready` has its tests run.
#[test]
fn a_failing_ready_before_the_run_runs_the_tests() {
    let repo = Repo::new("checks-ready-before");
    let c = checks(&repo);
    let tests = with(command("true"), "ready", Setting::Text("false".into()));
    let r = finish(&c, &request(&repo, "p1", tests));
    assert_eq!(r.tasks.len(), 1, "{:?}", r.tasks);
    assert_eq!(r.tasks[0].outcome, Outcome::Pass);
    assert!(r.not_run.is_empty() && r.missing.is_empty(), "{r:?}");
}

/// 84, could-not-run-1 114: nor after a run in which a check did not
/// pass, so its fail is recorded under its key.
#[test]
fn a_failing_ready_once_a_check_failed_leaves_the_fail() {
    let repo = Repo::new("checks-ready-after");
    let c = checks(&repo);
    std::fs::write(repo.dir().join("ready"), "").unwrap();
    let tests = with(
        command("rm ready; exit 1"),
        "ready",
        Setting::Text("test -e ready".into()),
    );
    let r = finish(&c, &request(&repo, "p1", tests));
    assert_eq!(fails(&r), ["rm ready; exit 1"]);
    assert!(r.not_run.is_empty(), "{:?}", r.not_run);
}

/// 84, 114: nothing of a `ready` command is run, before the run or after
/// a fail.
#[test]
fn a_ready_command_is_never_run() {
    let repo = Repo::new("checks-ready-never");
    let c = checks(&repo);
    let tests = with(
        command("exit 1"),
        "ready",
        Setting::Text("touch asked".into()),
    );
    let req = request(&repo, "p1", tests);
    let r = finish(&c, &req);
    assert_eq!(fails(&r), ["exit 1"]);
    for dir in [&req.place.path, &req.place.tmp] {
        assert!(!dir.join("asked").exists(), "{} holds asked", dir.display());
    }
}

/// What `toolchain` prints when it fails to enter.
const STALE_LOCK: &str = "error: flake.lock is not up to date";

/// `sluice` in `dir`, its state and configuration in `state`, out of
/// reach of the host's own and of every credential.
fn sluice_in(dir: &std::path::Path, state: &Scratch, args: &[&str]) -> std::process::Output {
    support::isolated(&mut std::process::Command::new(env!(
        "CARGO_BIN_EXE_sluice"
    )))
    .args(args)
    .current_dir(dir)
    // The host process its commands start passes only when asked, so the
    // test's own steps decide its rounds (135; design D14).
    .env("SLUICE_ROUND_SLEEP", "record")
    .env("XDG_CONFIG_HOME", state.path().join("config"))
    .env("XDG_STATE_HOME", state.path().join("state"))
    .env_remove("SLUICE_AGENT")
    .env_remove("SLUICE_OTEL")
    .env_remove("GIT_DIR")
    .env_remove("GIT_WORK_TREE")
    .stdin(std::process::Stdio::null())
    .output()
    .unwrap()
}

/// 16, 114, 115; the run-outcome spec's "The toolchain fails to enter"
/// and "Three starts that fail": `sluice step` on a combination whose
/// `[tests] toolchain` exits 1 records the run with the toolchain's words
/// and nothing under any key, three times, after which the combination is
/// not passed and `sluice status` names its tests could not run, with
/// those words.
#[test]
fn a_toolchain_that_fails_could_not_start_the_tests() {
    let repo = Repo::new("checks-could-not-start");
    repo.write(
        ".config/sluice.toml",
        &format!(
            "[tests]\nrunner = \"command\"\ncommand = \"true\"\n\
             toolchain = [\"sh\", \"-c\", \"echo '{STALE_LOCK}' >&2; exit 1\"]\n\n\
             [branches.\"bolt/*\"]\ntimeout = \"1m\"\n"
        ),
    );
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "sluice's policy"]);
    repo.git(&["branch", "bolt/x", "main"]);
    let sessions = Scratch::new("checks-could-not-start-sessions");
    let worktree = sessions.path().join("a");
    let at = worktree.to_str().unwrap();
    repo.git(&["worktree", "add", "-q", "-b", "unit/a", at, "bolt/x"]);
    std::fs::write(worktree.join("a.txt"), "a").unwrap();
    repo.git(&["-C", at, "add", "-A"]);
    repo.git(&["-C", at, "commit", "-q", "-m", "a"]);
    let state = Scratch::new("checks-could-not-start-state");
    let run = |args: &[&str]| {
        let out = sluice_in(&worktree, &state, args);
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        (out.status.code(), said)
    };
    let (code, said) = run(&["submit", "bolt/x", "--no-wait", "--json"]);
    assert_eq!(code, Some(0), "{said}");

    let words = format!("could not start: toolchain: exited 1: {STALE_LOCK}");
    let records = repo.dir().join(".git/sluice");
    let log = || std::fs::read_to_string(records.join("bolt/x/log.jsonl")).unwrap();
    let lost = |log: &str| {
        log.lines()
            .filter(|l| l.contains("\"lost\"") && l.contains(&words.replace('"', "\\\"")))
            .count()
    };
    let (code, said) = run(&["step", "bolt/x"]);
    assert_eq!(code, Some(0), "{said}");
    assert_eq!(lost(&log()), 1, "{}", log());
    let results = || std::fs::read_to_string(records.join("results.jsonl")).unwrap_or_default();
    assert_eq!(results(), "", "nothing under any key");
    let (_, said) = run(&["status", "1", "--json"]);
    assert_ne!(
        serde_json::from_str::<serde_json::Value>(&said).unwrap()["state"],
        "failed",
        "{said}"
    );

    for _ in 0..2 {
        let (code, said) = run(&["step", "bolt/x"]);
        assert_eq!(code, Some(0), "{said}");
    }
    let (code, said) = run(&["step", "bolt/x"]);
    assert_eq!(code, Some(0), "{said}");
    assert_eq!(lost(&log()), 2, "the third ends it: {}", log());
    assert_eq!(results(), "");
    let (_, said) = run(&["status", "1", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&said).unwrap();
    assert_eq!(v["state"], "failed", "{said}");
    assert_eq!(
        v["failed"],
        serde_json::json!([{"task": "[tests]", "not_run": words}]),
        "{said}"
    );
    let (_, said) = run(&["status", "1"]);
    assert!(said.contains("[tests] (could not run)"), "{said}");
}

/// What the stand-in `moon query tasks` answers: one task, `app:test`, run
/// in CI; with `--affected`, none.
const TASKS: &str = r#"{"tasks":{"app":{"test":{"target":"app:test","tags":[],"options":{}}}}}"#;

/// What the stand-in `moon setup` prints when it fails.
const NO_TOOLCHAIN: &str = "error: no such toolchain";

/// 147; rule core.46: a repository equipped by stand-ins for proto and
/// moon, which log each call to a file the test reads, and a toolchain of
/// the test's own that logs its call and puts them on the path. The
/// stand-in `moon` is absent until the stand-in `proto install` writes it,
/// where the repository pins its tools; otherwise it is there from the
/// start.
struct Equippable {
    repo: Repo,
    tools: Scratch,
}

impl Equippable {
    /// `pinned`: whether the repository has a `.prototools`; `setup`: what
    /// the stand-in `moon setup` runs.
    fn new(name: &str, pinned: bool, setup: &str) -> Equippable {
        use std::os::unix::fs::PermissionsExt as _;
        let repo = Repo::new(name);
        let tools = Scratch::new(&format!("{name}-tools"));
        let bin = tools.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let log = tools.path().join("log");
        let script = |path: &std::path::Path, body: String| {
            std::fs::write(path, format!("#!/bin/sh\n{body}")).unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        let moon = format!(
            "echo \"moon $*\" >> '{log}'\n\
             case \"$1\" in\n\
             --version) echo 'moon 2.6.1' ;;\n\
             setup) {setup} ;;\n\
             query) case \"$*\" in *--affected*) echo '{{\"tasks\":{{}}}}' ;; *) echo '{TASKS}' ;; esac ;;\n\
             esac\n",
            log = log.display(),
        );
        script(&tools.path().join("moon.in"), moon);
        script(
            &bin.join("proto"),
            format!(
                "echo \"proto $*\" >> '{log}'\n\
                 if [ \"$1\" = install ]; then cp '{tools}/moon.in' '{bin}/moon'; fi\n",
                log = log.display(),
                tools = tools.path().display(),
                bin = bin.display(),
            ),
        );
        script(
            &tools.path().join("toolchain"),
            format!(
                "echo toolchain >> '{log}'\necho 'export PATH=\"{bin}:$PATH\"'\n",
                log = log.display(),
                bin = bin.display(),
            ),
        );
        if pinned {
            repo.write(".prototools", "moon = \"2.6.1\"\n");
        } else {
            std::fs::copy(tools.path().join("moon.in"), bin.join("moon")).unwrap();
        }
        Equippable { repo, tools }
    }

    /// `[tests]` naming `runner` and the test's toolchain.
    fn tests(&self, runner: &str) -> Settings {
        let toolchain = self.tools.path().join("toolchain");
        Settings::from([
            ("runner".into(), Setting::Text(runner.into())),
            (
                "toolchain".into(),
                Setting::List(vec![toolchain.to_string_lossy().into()]),
            ),
        ])
    }

    /// The working copy equipped through `through` by `tests`.
    fn equip(&self, tests: Settings, through: Through) -> Vec<EquipStep> {
        checks(&self.repo)
            .equip(&Equip {
                working_copy: self.repo.dir().to_path_buf(),
                tests,
                through,
            })
            .unwrap()
            .steps
    }

    /// Each call the stand-ins and the toolchain logged, in order.
    fn log(&self) -> Vec<String> {
        std::fs::read_to_string(self.tools.path().join("log"))
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    }
}

/// The steps' names and whether each passed.
fn outcomes(steps: &[EquipStep]) -> Vec<(&str, bool)> {
    steps.iter().map(|s| (s.step.as_str(), s.passed)).collect()
}

/// 147; rule core.46: a working copy that pins its tools, nothing of them
/// installed, is equipped by the toolchain, `proto install`, `moon
/// --version` and `moon setup`, in that order, each passing.
#[test]
fn equip_runs_the_sequence_in_order() {
    let w = Equippable::new("equip-order", true, "true");
    let steps = w.equip(w.tests("moon"), Through::Running);
    assert_eq!(
        outcomes(&steps),
        [
            ("toolchain", true),
            ("proto install", true),
            ("moon --version", true),
            ("moon setup", true),
        ]
    );
    assert_eq!(
        w.log(),
        [
            "toolchain",
            "proto install --config-mode upwards",
            "moon --version",
            "moon setup",
        ]
    );
}

/// 147; rule core.46: a working copy with no `.prototools` runs no `proto
/// install` and names none, `moon --version` following the toolchain;
/// equipped only for reading, it is never set up.
#[test]
fn equip_installs_only_what_is_pinned() {
    let w = Equippable::new("equip-unpinned", false, "true");
    let steps = w.equip(w.tests("moon"), Through::Reading);
    assert_eq!(
        outcomes(&steps),
        [("toolchain", true), ("moon --version", true)]
    );
    let steps = w.equip(w.tests("moon"), Through::Running);
    assert_eq!(
        outcomes(&steps),
        [
            ("toolchain", true),
            ("moon --version", true),
            ("moon setup", true),
        ]
    );
    assert!(
        !w.log().iter().any(|l| l.starts_with("proto")),
        "{:?}",
        w.log()
    );
}

/// 147; rule core.46: under the command runner the toolchain is the one
/// step, even where the working copy pins its tools; with no toolchain
/// there is no step.
#[test]
fn equip_under_the_command_runner_runs_the_toolchain_alone() {
    let w = Equippable::new("equip-command", true, "true");
    let steps = w.equip(w.tests("command"), Through::Running);
    assert_eq!(outcomes(&steps), [("toolchain", true)]);
    assert_eq!(w.log(), ["toolchain"]);
    let bare = Settings::from([("runner".into(), Setting::Text("command".into()))]);
    assert_eq!(w.equip(bare, Through::Running), []);
}

/// 147; rule core.46: the sequence stops at the step that failed, which
/// is given its exit and what it printed.
#[test]
fn equip_stops_at_the_step_that_failed() {
    let w = Equippable::new(
        "equip-stops",
        true,
        &format!("echo '{NO_TOOLCHAIN}' >&2; exit 1"),
    );
    let steps = w.equip(w.tests("moon"), Through::Running);
    assert_eq!(
        outcomes(&steps),
        [
            ("toolchain", true),
            ("proto install", true),
            ("moon --version", true),
            ("moon setup", false),
        ]
    );
    assert_eq!(steps[3].words, format!("exited 1: {NO_TOOLCHAIN}"));
}

/// 147; rule core.46: a toolchain that fails is the one step, its words
/// its exit and its error stream, and nothing of the runner runs.
#[test]
fn a_toolchain_that_fails_runs_nothing_after_it() {
    let w = Equippable::new("equip-toolchain-fails", true, "true");
    let log = w.tools.path().join("log");
    let mut tests = w.tests("moon");
    tests.insert(
        "toolchain".into(),
        Setting::List(vec![
            "sh".into(),
            "-c".into(),
            format!(
                "echo toolchain >> '{}'; echo '{STALE_LOCK}' >&2; exit 1",
                log.display()
            ),
        ]),
    );
    assert_eq!(
        w.equip(tests, Through::Running),
        [EquipStep {
            step: "toolchain".into(),
            passed: false,
            words: format!("exited 1: {STALE_LOCK}"),
        }]
    );
    assert_eq!(w.log(), ["toolchain"]);
}

/// 147, 71; rule core.46: an equip keeps nothing once it answers: the
/// directory its toolchain's script went in is gone, and nothing of its
/// own is left among the check seam's runs.
#[test]
fn equip_keeps_nothing() {
    let w = Equippable::new("equip-keeps-nothing", true, "true");
    let was = w.tools.path().join("tmp-was");
    let mut tests = w.tests("moon");
    tests.insert(
        "toolchain".into(),
        Setting::List(vec![
            "sh".into(),
            "-c".into(),
            format!(
                "echo \"$SLUICE_TMP\" > '{}'; echo 'export PATH=\"{}:$PATH\"'",
                was.display(),
                w.tools.path().join("bin").display()
            ),
        ]),
    );
    let steps = w.equip(tests, Through::Running);
    assert!(steps.iter().all(|s| s.passed), "{steps:?}");
    let tmp = std::fs::read_to_string(&was).unwrap();
    let tmp = std::path::Path::new(tmp.trim());
    let runs = w.repo.dir().join(".git/sluice/.runs");
    assert!(tmp.starts_with(&runs), "{}", tmp.display());
    assert!(!tmp.exists(), "{} is left", tmp.display());
    let left: Vec<String> = std::fs::read_dir(&runs)
        .map(|d| {
            d.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    assert!(left.is_empty(), "{left:?}");
}

/// Run in a child of this test binary, on a host that declares a cache's
/// variables, with the caller's environment holding one sluice does not
/// keep.
#[test]
fn equip_env_probe() {
    let Ok(out) = std::env::var("SLUICE_TEST_EQUIP_ENV_OUT") else {
        return;
    };
    let repo = Repo::new("equip-env");
    let c = caching(
        &repo,
        vec![HostCache {
            dir: "/caches/c".into(),
            socket: None,
            env: [("CACHE_DIR".to_string(), "/caches/c".to_string())].into(),
        }],
    );
    let tests = Settings::from([
        ("runner".into(), Setting::Text("command".into())),
        (
            "toolchain".into(),
            Setting::List(vec!["sh".into(), "-c".into(), "env >&2".into()]),
        ),
    ]);
    let equipped = c
        .equip(&Equip {
            working_copy: repo.dir().to_path_buf(),
            tests,
            through: Through::Running,
        })
        .unwrap();
    std::fs::write(&out, &equipped.steps[0].words).unwrap();
}

/// 147, 35; rule core.46: each step gets the environment a run starts
/// from, the host's caches' variables and the working copy's paths among
/// it, and nothing else of the caller's.
#[test]
fn equip_steps_get_a_runs_environment() {
    let out = support::Scratch::new("equip-env-out");
    let file = out.path().join("seen");
    let status = support::isolated(&mut std::process::Command::new(
        std::env::current_exe().unwrap(),
    ))
    .args(["--exact", "equip_env_probe", "--nocapture"])
    .env("SLUICE_TEST_EQUIP_ENV_OUT", &file)
    .env("FROM_THE_CALLER", "1")
    .status()
    .unwrap();
    assert!(status.success());
    let seen = std::fs::read_to_string(&file).unwrap();
    assert!(seen.lines().any(|l| l == "CACHE_DIR=/caches/c"), "{seen}");
    assert!(
        seen.lines().any(|l| l.starts_with("SLUICE_CHECKOUT=")),
        "{seen}"
    );
    assert!(!seen.contains("FROM_THE_CALLER"), "{seen}");
}

/// A run's request of `w`'s working copy under the moon runner, requiring
/// `app:test`.
fn moon_request(w: &Equippable, name: &str) -> RunRequest {
    let mut req = request(&w.repo, name, w.tests("moon"));
    req.required = vec!["app:test".into()];
    req
}

/// 147; rule core.46: a combination that affects a required check is set
/// up after moon's queries and before the check runs.
#[test]
fn a_run_that_runs_a_check_is_set_up() {
    let w = Equippable::new("equip-run-set-up", true, "true");
    let c = checks(&w.repo);
    finish(&c, &moon_request(&w, "p1"));
    let log = w.log();
    let at = |what: &str| {
        log.iter()
            .position(|l| l.starts_with(what))
            .unwrap_or_else(|| panic!("no {what}: {log:?}"))
    };
    assert_eq!(
        log[..3],
        [
            "toolchain",
            "proto install --config-mode upwards",
            "moon --version"
        ]
    );
    assert!(at("moon query tasks") < at("moon setup"), "{log:?}");
    assert!(at("moon setup") < at("moon exec app:test"), "{log:?}");
}

/// 147; rule core.46: a combination that affects no required check passes
/// with nothing run, and its working copy is never set up.
#[test]
fn a_run_that_runs_nothing_is_never_set_up() {
    let w = Equippable::new("equip-run-nothing", true, "true");
    let c = checks(&w.repo);
    let mut req = moon_request(&w, "p1");
    req.since = Some(req.revision.clone());
    let r = finish(&c, &req);
    assert!(r.tasks.is_empty() && r.missing.is_empty(), "{r:?}");
    let log = w.log();
    assert!(
        log.iter()
            .any(|l| l.starts_with("moon query tasks --affected")),
        "{log:?}"
    );
    assert!(!log.iter().any(|l| l == "moon setup"), "{log:?}");
}

/// 89, 98, 147, S72; rule core.46: a session's or a preview's command
/// runs in place of the runner's checks, in a working copy equipped
/// through the setup by the runner its branch's policy names.
#[test]
fn a_run_with_a_command_is_equipped_by_its_branchs_runner() {
    let w = Equippable::new("equip-command", true, "true");
    let c = checks(&w.repo);
    let command = format!("echo command >> '{}'", w.tools.path().join("log").display());
    let mut req = request(&w.repo, "p1", w.tests("moon"));
    req.command = Some(command.clone());
    let r = finish(&c, &req);
    assert_eq!(r.tasks.len(), 1, "{r:?}");
    assert_eq!(r.tasks[0].task, command);
    assert_eq!(r.tasks[0].outcome, Outcome::Pass);
    assert_eq!(
        w.log(),
        [
            "toolchain",
            "proto install --config-mode upwards",
            "moon --version",
            "moon setup",
            "command",
        ]
    );
}

/// 147, record 0050; rule core.46: a submitter's worktree, read for the
/// checks moon keeps out of CI, is equipped up to the version check and
/// never set up.
#[test]
fn the_submitters_read_is_never_set_up() {
    let w = Equippable::new("equip-submitters-read", true, "true");
    let kept = checks(&w.repo)
        .kept_out(w.repo.dir(), &w.tests("moon"), &["app:test".into()])
        .unwrap();
    assert!(kept.is_empty(), "{kept:?}");
    assert_eq!(
        w.log(),
        [
            "toolchain",
            "proto install --config-mode upwards",
            "moon --version",
            "moon query tasks",
        ]
    );
}

/// 147, 114; rule core.46: a run whose working copy could not be
/// equipped starts nothing, and fails its start with the step's name and
/// words.
#[test]
fn a_run_whose_working_copy_could_not_be_equipped_could_not_start() {
    let w = Equippable::new(
        "equip-could-not-start",
        true,
        &format!("echo '{NO_TOOLCHAIN}' >&2; exit 1"),
    );
    let c = checks(&w.repo);
    let err = support::start(&c, &moon_request(&w, "p1")).unwrap_err();
    assert_eq!(
        format!("{err:#}"),
        format!("moon setup: exited 1: {NO_TOOLCHAIN}")
    );
    assert!(!c.runs().dir("p1").join("request.json").exists());
    let log = w.log();
    assert!(!log.iter().any(|l| l.starts_with("moon exec")), "{log:?}");
}

/// A preview's place under `dir`, as the place profile hands it to the
/// end of its containers: its checkout, made, its scratch, its `web` port
/// and its `SLUICE_PLACE` (record 0121).
fn preview_place(dir: &Path) -> Place {
    let path = dir.join("previews/preview+2F0123456789ab");
    std::fs::create_dir_all(path.join("tls")).unwrap();
    Place {
        name: "preview/0123456789ab".into(),
        path,
        tmp: dir.join("scratch/0123456789"),
        env: [
            ("SLUICE_PLACE", "sluice-0a1b2c3d4e"),
            ("SLUICE_PORT_WEB", "20000"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect(),
    }
}

/// Record 0121's four containers for `place`: one mounting a file of its
/// checkout, one publishing its `web` port, one labelled with its
/// `SLUICE_PLACE`, and one of another checkout that does none of these.
fn four_containers(place: &Place, dir: &Path) -> serde_json::Value {
    let other = dir.join("previews/preview+2F0123456789ab-other/tls");
    serde_json::json!([
        support::container("mounts", &[&place.path.join("tls/cert.pem")], &[], &[]),
        support::container("publishes", &[], &[20000], &[]),
        support::container(
            "labelled",
            &[],
            &[],
            &[(sluice::profiles::checks::PLACE_LABEL, "sluice-0a1b2c3d4e")]
        ),
        support::container(
            "other",
            &[&other],
            &[20001],
            &[("sluice.place", "sluice-ffffffffff")]
        ),
    ])
}

/// 34, previews-1 clause 101, record 0121 (design D2, D4), the
/// preview-release spec's "Another checkout's container is left" at the
/// check profile's level: a container that mounts the checkout, one that
/// publishes its port and one labelled with its place are removed, and
/// one of another checkout is left; a link to the engine's socket is the
/// same engine, asked once.
#[test]
fn the_places_containers_are_removed_and_no_other() {
    let dir = Scratch::short();
    let engine = support::containers_engine(dir.path());
    let place = preview_place(dir.path());
    engine.hold(four_containers(&place, dir.path()));
    let link = dir.path().join("l.sock");
    std::os::unix::fs::symlink(engine.socket(), &link).unwrap();

    sluice::profiles::checks::end_containers(
        &[Endpoint::Unix(link), Endpoint::Unix(engine.socket())],
        &place,
    )
    .unwrap();
    assert_eq!(engine.removed(), ["mounts", "publishes", "labelled"]);
    assert_eq!(engine.listings(), 1, "one engine, asked once");
}

/// Record 0121 (design D4): an engine that answers in HTTP/1.1 and holds
/// each connection open after its answer, as one host's did (2026-11-11),
/// is read by the end each answer gives itself, its last chunk or a
/// `204`, and never waited on: its refusal's words are read from its
/// chunks, the rest removed, and the next end removes what it refused.
#[test]
fn an_engine_that_keeps_its_connections_open_is_read_to_each_answers_end() {
    let dir = Scratch::short();
    let engine = support::containers_engine(dir.path());
    engine.keep_open();
    let place = preview_place(dir.path());
    engine.hold(four_containers(&place, dir.path()));
    engine.refuse("labelled", 1);
    let at = [Endpoint::Unix(engine.socket())];

    let began = std::time::Instant::now();
    let e = sluice::profiles::checks::end_containers(&at, &place)
        .unwrap_err()
        .to_string();
    assert!(
        e.contains(&format!("labelled: {}", support::REFUSED.1)),
        "{e}"
    );
    assert_eq!(engine.removed(), ["mounts", "publishes"]);
    sluice::profiles::checks::end_containers(&at, &place).unwrap();
    assert_eq!(engine.removed(), ["mounts", "publishes", "labelled"]);
    assert!(
        began.elapsed() < std::time::Duration::from_secs(5),
        "waited {:?} on connections the engine held",
        began.elapsed()
    );
}

/// Record 0121 (design D5): a removal the engine refuses is named with
/// the engine's words, the other containers are removed all the same, and
/// one the engine says is gone counts as removed.
#[test]
fn a_removal_the_engine_refuses_is_named_and_the_rest_go_on() {
    let dir = Scratch::short();
    let engine = support::containers_engine(dir.path());
    let place = preview_place(dir.path());
    let shown = four_containers(&place, dir.path());
    let asked = std::sync::atomic::AtomicU32::new(0);
    engine.hold_with(move || {
        let mut all = shown.clone();
        // Listed, then gone before its removal: the engine answers 404.
        if asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
            let vanished = support::container("vanished", &[], &[20000], &[]);
            all.as_array_mut().unwrap().push(vanished);
        }
        all
    });
    engine.refuse("mounts", 1);

    let e = sluice::profiles::checks::end_containers(&[Endpoint::Unix(engine.socket())], &place)
        .unwrap_err()
        .to_string();
    assert_eq!(
        e,
        format!(
            "could not remove the containers preview/0123456789ab left: mounts: {}",
            support::REFUSED.1
        )
    );
    assert_eq!(engine.removed(), ["publishes", "labelled"]);

    // The next try removes it.
    sluice::profiles::checks::end_containers(&[Endpoint::Unix(engine.socket())], &place).unwrap();
    assert_eq!(engine.removed(), ["publishes", "labelled", "mounts"]);
}

/// Record 0121 (design D4): where no engine answers, there is nothing to
/// end: no endpoint, a socket path with nothing at it, and an engine that
/// answers its ping with a 500 are each passed over.
#[test]
fn a_host_with_no_engine_has_no_container_to_end() {
    let dir = Scratch::short();
    let place = preview_place(dir.path());
    let end = |endpoints: &[Endpoint]| sluice::profiles::checks::end_containers(endpoints, &place);
    end(&[]).unwrap();
    end(&[Endpoint::Unix(dir.path().join("none.sock"))]).unwrap();
    let failing = Scratch::short();
    let socket = support::engine(failing.path(), "HTTP/1.0 500 Internal Server Error");
    end(&[Endpoint::Unix(socket)]).unwrap();
}

/// Record 0121 (design D4): an engine at a TCP address is listed and its
/// containers removed as one on a socket is.
#[test]
fn a_tcp_engine_is_asked_as_a_socket_is() {
    let dir = Scratch::short();
    let engine = support::tcp_containers_engine();
    let place = preview_place(dir.path());
    engine.hold(four_containers(&place, dir.path()));
    engine.refuse("labelled", 1);
    let at = Endpoint::Tcp {
        host: "127.0.0.1".into(),
        port: engine.port(),
        tls: None,
    };
    let e = sluice::profiles::checks::end_containers(std::slice::from_ref(&at), &place)
        .unwrap_err()
        .to_string();
    assert!(
        e.ends_with(&format!("labelled: {}", support::REFUSED.1)),
        "{e}"
    );
    assert_eq!(engine.removed(), ["mounts", "publishes"]);
    sluice::profiles::checks::end_containers(&[at], &place).unwrap();
    assert_eq!(engine.removed(), ["mounts", "publishes", "labelled"]);
    assert_eq!(engine.listings(), 2);
}
