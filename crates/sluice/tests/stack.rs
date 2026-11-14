//! A full stack per candidate, side by side (31, 32, 34, 66; ADR 0016):
//! each candidate's check starts a container on one of its place's ports
//! and a server on another, then a browser test against both, while
//! another candidate's stack runs beside it. The release command removes
//! the containers; a candidate ended by its time limit leaves no container
//! and no process. Needs git, a container engine, bun and Playwright with
//! its browser (sluice's devenv shell), so it is in the check-tools tier;
//! the sandbox's own run of it is in the sandbox task, whose tests start
//! a sandbox.

#![cfg(feature = "tools")]

#[path = "support/mod.rs"]
mod support;

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use support::{Repo, Scratch};

/// The check: a container and a server on the place's ports, both up and
/// the other candidate's container up beside them, then the browser test.
/// A candidate holding `hang` starts its stack and never finishes.
const CHECK: &str = r#"set -eu
docker run -d --rm --label "sluice.fixture=$FIXTURE" --label "sluice.place=$SLUICE_PLACE" \
  -p "127.0.0.1:$SLUICE_PORT_WEB:80" nginx:alpine >/dev/null
bun --eval 'Bun.serve({ hostname: "127.0.0.1", port: Number(process.env.SLUICE_PORT_API),
  fetch: () => new Response("<h1>" + process.env.SLUICE_PLACE + "</h1>",
    { headers: { "content-type": "text/html" } }) })' "api-$FIXTURE" &
up() {
  n=0
  until curl -fs -o /dev/null "http://127.0.0.1:$1/"; do
    n=$((n + 1)); [ "$n" -lt 300 ] || { echo "nothing answers on $1"; exit 1; }
    sleep 0.2
  done
}
up "$SLUICE_PORT_WEB"
up "$SLUICE_PORT_API"
if [ -f hang ]; then sleep 600; fi
n=0
until [ "$(docker ps -q --filter "label=sluice.fixture=$FIXTURE" | wc -l)" -ge 2 ]; do
  n=$((n + 1)); [ "$n" -lt 450 ] || { echo "the other stack never came up"; exit 1; }
  sleep 0.2
done
playwright test
"#;

const RELEASE: &str = r#"ids=$(docker ps -aq --filter "label=sluice.place=$SLUICE_PLACE")
[ -z "$ids" ] || docker rm -f $ids >/dev/null
"#;

const SPEC: &str = r#"const { test, expect } = require('@playwright/test');
test('the stack', async ({ page }) => {
  await page.goto(`http://127.0.0.1:${process.env.SLUICE_PORT_WEB}/`);
  await expect(page).toHaveTitle('Welcome to nginx!');
  await page.goto(`http://127.0.0.1:${process.env.SLUICE_PORT_API}/`);
  await expect(page.locator('h1')).toHaveText(process.env.SLUICE_PLACE);
});
"#;

const CONFIG: &str = r#"module.exports = {
  use: { browserName: 'chromium', channel: 'chromium' },
  reporter: 'line',
  outputDir: process.env.SLUICE_TMP + '/playwright',
};
"#;

fn policy(executor: &str) -> String {
    format!(
        "[tests]\nrunner = \"command\"\ncommand = \"sh check.sh\"\nrelease = \"sh release.sh\"\n\n\
         [checkout]\nports = [\"web\", \"api\"]\n\n\
         [executor]\nkind = \"{executor}\"\ndocker = true\n{network}\n\
         [branches.\"stack/*\"]\nparallel = 2\ntimeout = \"3m\"\n\n\
         [branches.\"hang/*\"]\ntimeout = \"20s\"\n",
        network = if executor == "sandbox" {
            "network = \"localhost\"\n"
        } else {
            ""
        },
    )
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
    config: Scratch,
    state: Scratch,
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

    /// A session on `unit/<name>` off `branch`, with one commit adding
    /// `file`, submitted without waiting.
    fn submit(&self, branch: &str, name: &str, file: &str) {
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
        std::fs::write(dir.join(file), name).unwrap();
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

    /// The branch's status, for a failed assertion's message.
    fn status(&self, branch: &str) -> String {
        let out = self
            .command(self.repo.dir())
            .args(["status", branch, "--json", "--all"])
            .output()
            .unwrap();
        let mut t = text(&out);
        let status: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
        for log in status
            .to_string()
            .split('"')
            .filter(|p| p.ends_with(".log"))
        {
            t.push_str(&format!(
                "\n--- {log}\n{}",
                std::fs::read_to_string(log).unwrap_or_default()
            ));
        }
        t
    }

    fn states(&self, branch: &str) -> Vec<String> {
        let out = self
            .command(self.repo.dir())
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
}

/// What is left of the fixture `id`: its containers, and its servers.
fn leftovers(id: &str) -> (String, String) {
    let containers = Command::new("docker")
        .args([
            "ps",
            "-aq",
            "--filter",
            &format!("label=sluice.fixture={id}"),
        ])
        .output()
        .expect("docker on PATH");
    let servers = Command::new("pgrep")
        .args(["-f", &format!("api-{id}")])
        .output()
        .expect("pgrep");
    (
        String::from_utf8_lossy(&containers.stdout)
            .trim()
            .to_string(),
        String::from_utf8_lossy(&servers.stdout).trim().to_string(),
    )
}

/// The fixture `id`, whose containers go when its test ends, so a test
/// gone red leaves none of them running.
struct Fixture(String);

impl Drop for Fixture {
    fn drop(&mut self) {
        let Ok(out) = Command::new("docker")
            .args([
                "ps",
                "-aq",
                "--filter",
                &format!("label=sluice.fixture={}", self.0),
            ])
            .output()
        else {
            return;
        };
        let ids = String::from_utf8_lossy(&out.stdout);
        let ids: Vec<&str> = ids.split_whitespace().collect();
        if !ids.is_empty() {
            let _ = Command::new("docker")
                .args(["rm", "-f"])
                .args(&ids)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}

fn stacks(executor: &str) {
    let id = format!("{}-{executor}", std::process::id());
    let _fixture = Fixture(id.clone());
    let repo = Repo::new("stack");
    let check = format!(
        "export FIXTURE={id}\nexport PLAYWRIGHT_BROWSERS_PATH={}\n{CHECK}",
        std::env::var("PLAYWRIGHT_BROWSERS_PATH")
            .expect("PLAYWRIGHT_BROWSERS_PATH, from sluice's devenv shell"),
    );
    repo.write(".config/sluice.toml", &policy(executor));
    repo.write("check.sh", &check);
    repo.write("release.sh", RELEASE);
    repo.write("stack.spec.js", SPEC);
    repo.write("playwright.config.js", CONFIG);
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "a stack per candidate"]);
    for b in ["stack/a", "stack/b", "hang/c"] {
        repo.git(&["branch", b, "main"]);
    }
    let config = Scratch::new("stack-config");
    std::fs::create_dir_all(config.path().join("sluice")).unwrap();
    std::fs::write(
        config.path().join("sluice/config.toml"),
        "[runs]\nmax = 3\n",
    )
    .unwrap();
    let s = Setup {
        repo,
        sessions: Scratch::new("stack-sessions"),
        config,
        state: Scratch::new("stack-state"),
    };
    s.submit("stack/a", "a", "a.txt");
    s.submit("stack/b", "b", "b.txt");
    s.submit("hang/c", "c", "hang");

    // Rounds until all three are decided, however slowly a loaded host
    // brings the stacks up: bounded by time, generously, never by a count
    // of rounds.
    let decided = |b: &str| {
        let states = s.states(b);
        states.len() == 1 && ["landed", "failed", "conflict", "withdrawn"].contains(&&*states[0])
    };
    let until = Instant::now() + Duration::from_secs(20 * 60);
    let out = loop {
        let out = s
            .command(s.repo.dir())
            .args(["run", "--rounds", "10"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", text(&out));
        if ["stack/a", "stack/b", "hang/c"].iter().all(|b| decided(b)) {
            break out;
        }
        assert!(
            Instant::now() < until,
            "not decided in 20 minutes: {}\n{}\n{}",
            s.status("stack/a"),
            s.status("stack/b"),
            s.status("hang/c")
        );
    };

    // 31: two stacks on their own ports, up at once, each test reaching
    // its own; both land.
    assert_eq!(s.states("stack/a"), ["landed"], "{}", s.status("stack/a"));
    assert_eq!(s.states("stack/b"), ["landed"], "{}", s.status("stack/b"));
    // 66: the hung candidate was ended by its time limit.
    assert_eq!(s.states("hang/c"), ["failed"], "{}", text(&out));
    // 32, 34: nothing any of them started is left: no container, no
    // server; and every block of ports is free again.
    assert_eq!(leftovers(&id), (String::new(), String::new()));
    let blocks = s.state.path().join("sluice/ports");
    let held: Vec<String> = std::fs::read_dir(&blocks)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with('.'))
        .collect();
    assert!(held.is_empty(), "blocks of ports still held: {held:?}");
}

#[test]
fn two_stacks_side_by_side_and_a_time_limit_leaves_nothing() {
    stacks("open");
}

/// The same under the sandbox, which lets the run reach the container
/// engine and start a browser, and holds its network to localhost.
#[test]
#[cfg(target_os = "macos")]
fn two_stacks_side_by_side_in_the_sandbox() {
    stacks("sandbox");
}

/// A session run's check: the stack is brought up once, on the worktree's
/// port, and found by every later run (session-runs-1 clause 94).
const SESSION_CHECK: &str = r#"set -eu
if [ -n "$(docker ps -q --filter "label=sluice.place=$SLUICE_PLACE")" ]; then
  echo found >> "$SLUICE_CHECKOUT/runs.log"
else
  docker run -d --rm --label "sluice.fixture=$FIXTURE" --label "sluice.place=$SLUICE_PLACE" \
    -p "127.0.0.1:$SLUICE_PORT_WEB:80" nginx:alpine >/dev/null
  echo started >> "$SLUICE_CHECKOUT/runs.log"
fi
n=0
until curl -fs -o /dev/null "http://127.0.0.1:$SLUICE_PORT_WEB/"; do
  n=$((n + 1)); [ "$n" -lt 300 ] || { echo "nothing answers"; exit 1; }
  sleep 0.2
done
"#;

/// Session-runs-1 clauses 90, 94, 95: a session's worktree adopted twice
/// keeps its ports, and the container its first run started is still up
/// after the run and found by the second; its release runs the release
/// command once, which removes the container, and frees the ports.
#[test]
fn a_sessions_stack_stays_between_runs_and_goes_on_release() {
    use sluice::instance::Instance;
    use sluice::profiles::checks::Checks;
    use sluice::profiles::places::Places;
    use sluice::runs::Runs;
    use sluice_seams::{Checkout, Limits, Progress, Rev, RunRequest, Setting, Settings, Tests};

    let id = format!("{}-session", std::process::id());
    let _fixture = Fixture(id.clone());
    let repo = Repo::new("stack-session");
    repo.write("check.sh", &format!("export FIXTURE={id}\n{SESSION_CHECK}"));
    repo.write("release.sh", RELEASE);
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "a stack for a session"]);
    let worktree = Scratch::new("stack-session-worktree");
    let wt = worktree.path().join("a");
    repo.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "unit/a",
        wt.to_str().unwrap(),
        "main",
    ]);
    let head = Rev::new(repo.rev("unit/a"));
    let common = repo.dir().join(".git");
    let program = PathBuf::from(env!("CARGO_BIN_EXE_sluice"));
    let blocks = Scratch::new("stack-session-blocks");
    let running = Scratch::new("stack-session-running");
    let places = Places::new(&common, Runs::new(&common, program.clone()))
        .with_ports(blocks.path().to_path_buf())
        .with_scratch_root(support::test_user_scratch());
    let checks = Checks::new(
        &common,
        program,
        Instance {
            bound: 2,
            reserved: 0,
            processors: 2,
            forge: None,
            signing: None,
            peers: Default::default(),
            systems: None,
            caches: Vec::new(),
            expire_after: sluice_core::EXPIRE_AFTER,
            telemetry: None,
        },
    )
    .with_running(running.path().to_path_buf());
    let checkout = Settings::from([
        ("ports".into(), Setting::List(vec!["web".into()])),
        ("ports_from".into(), Setting::Text("41400".into())),
    ]);
    let tests = Settings::from([
        ("runner".into(), Setting::Text("command".into())),
        ("command".into(), Setting::Text("sh check.sh".into())),
        ("release".into(), Setting::Text("sh release.sh".into())),
    ]);
    let run = |n: usize| {
        let place = places
            .adopt(
                wt.to_str().unwrap(),
                "bolt/x",
                &checkout,
                "2026-10-14T09:00:00Z",
            )
            .unwrap();
        let handle = checks
            .start(&RunRequest {
                place: place.clone(),
                revision: head.clone(),
                required: vec!["sh check.sh".into()],
                since: None,
                limit: Some(std::time::Duration::from_secs(180)),
                limits: Limits {
                    docker: true,
                    ..Limits::default()
                },
                tests: tests.clone(),
                command: None,
                replay: false,
                system: None,
            })
            .unwrap();
        let until = std::time::Instant::now() + std::time::Duration::from_secs(200);
        let report = loop {
            match checks.progress(&handle).unwrap() {
                Progress::Running => {
                    assert!(std::time::Instant::now() < until, "run {n} never ended");
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
                Progress::Finished(r) => break r,
                Progress::Lost => panic!("run {n} was lost"),
            }
        };
        assert!(
            report
                .tasks
                .iter()
                .all(|t| t.outcome == sluice_seams::Outcome::Pass),
            "run {n}: {report:?}\n{}",
            std::fs::read_to_string(checks.runs().dir(&handle.0).join("out.log"))
                .unwrap_or_default()
        );
        place
    };

    let first = run(1);
    // 31, 90: the port is its block's, from `ports_from`, 41400, on; the
    // block is the first whose port nothing on the host already holds, a
    // run of this test in another tree's among them.
    let held: Vec<u32> = std::fs::read_dir(blocks.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with('.'))
        .map(|n| n.parse().unwrap())
        .collect();
    assert_eq!(held.len(), 1, "blocks of ports held: {held:?}");
    assert_eq!(first.env["SLUICE_PORT_WEB"], (41400 + held[0]).to_string());
    // 94: the run ended and its container did not.
    assert!(!leftovers(&id).0.is_empty(), "the stack went with the run");
    let second = run(2);
    assert_eq!(second, first, "the same place and ports on the second run");
    assert_eq!(
        std::fs::read_to_string(wt.join("runs.log")).unwrap(),
        "started\nfound\n"
    );
    assert!(!leftovers(&id).0.is_empty());

    // 95: released, the container is gone, the ports free, the worktree
    // standing.
    places.release(&first.name).unwrap();
    assert_eq!(leftovers(&id).0, "", "the release left a container");
    let held: Vec<String> = std::fs::read_dir(blocks.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with('.'))
        .collect();
    assert!(held.is_empty(), "blocks of ports still held: {held:?}");
    assert!(wt.join("check.sh").exists());
    assert!(repo
        .git(&["worktree", "list"])
        .contains(wt.to_str().unwrap()));
}

/// What a preview's command brings up: a container on the preview's port,
/// handed off to the engine; then it says what it was given and exits.
const PREVIEW: &str = r#"set -eu
docker run -d --rm --label "sluice.fixture=$FIXTURE" --label "sluice.place=$SLUICE_PLACE" \
  -p "127.0.0.1:$SLUICE_PORT_WEB:80" nginx:alpine >/dev/null
n=0
until curl -fs -o /dev/null "http://127.0.0.1:$SLUICE_PORT_WEB/"; do
  n=$((n + 1)); [ "$n" -lt 300 ] || { echo "nothing answers on $SLUICE_PORT_WEB"; exit 1; }
  sleep 0.2
done
echo "$SLUICE_NAME_WEB $SLUICE_PORT_WEB $SLUICE_BRANCH $SLUICE_REVISION" > given
"#;

/// Previews-1 clauses 98, 99, 101, 102: a preview's command starts a
/// container on its port and exits; the container is still up after, its
/// name and port reached the command, the host counts nothing once the
/// command has ended, and its release removes the container and frees the
/// port.
#[test]
fn a_previews_stack_stays_up_until_it_is_released() {
    let id = format!("{}-preview", std::process::id());
    let _fixture = Fixture(id.clone());
    let repo = Repo::new("stack-preview");
    repo.write(
        ".config/sluice.toml",
        "[tests]\nrunner = \"command\"\ncommand = \"true\"\nrelease = \"sh release.sh\"\n\n\
         [checkout]\nports = [\"web\"]\nports_from = \"41900\"\n\n\
         [executor]\ndocker = true\n\n\
         [branches.\"bolt/*\"]\ntimeout = \"3m\"\n\n\
         [preview]\ncommand = \"sh preview.sh\"\n\
         addresses = { web = \"https://{name.web}.localhost\" }\n",
    );
    repo.write("preview.sh", &format!("export FIXTURE={id}\n{PREVIEW}"));
    repo.write("release.sh", RELEASE);
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "a preview"]);
    repo.git(&["branch", "bolt/x", "main"]);
    let rev = repo.rev("bolt/x");
    let setup = Setup {
        repo,
        sessions: Scratch::new("stack-preview-sessions"),
        config: Scratch::new("stack-preview-config"),
        state: Scratch::new("stack-preview-state"),
    };
    let sluice = |args: &[&str]| setup.command(setup.repo.dir()).args(args).output().unwrap();

    let out = sluice(&["preview", "bolt/x", &rev[..10], "--wait", "--json"]);
    assert!(out.status.success(), "{}", text(&out));
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["state"], "up", "{doc}");
    let name = doc["names"]["SLUICE_NAME_WEB"]
        .as_str()
        .unwrap()
        .to_string();
    let port = doc["ports"]["SLUICE_PORT_WEB"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(doc["addresses"]["web"], format!("https://{name}.localhost"));
    let checkout = PathBuf::from(doc["checkout"].as_str().unwrap());
    assert_eq!(
        std::fs::read_to_string(checkout.join("given")).unwrap(),
        format!("{name} {port} bolt/x {rev}\n")
    );
    // 98: handed off, the container outlived the command.
    assert!(
        !leftovers(&id).0.is_empty(),
        "the stack went with the command"
    );
    let answer = Command::new("curl")
        .args([
            "-fs",
            "-o",
            "/dev/null",
            &format!("http://127.0.0.1:{port}/"),
        ])
        .status()
        .unwrap();
    assert!(
        answer.success(),
        "nothing answers on {port} after the command"
    );
    // 102: the host counts nothing once the command has ended.
    let counted = std::fs::read_dir(setup.state.path().join("sluice/sessions"))
        .map(|d| {
            d.filter(|e| {
                !e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with('.')
            })
            .count()
        })
        .unwrap_or(0);
    assert_eq!(counted, 0, "a held preview still counts");

    let out = sluice(&["release", doc["name"].as_str().unwrap()]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(leftovers(&id).0, "", "the release left a container");
    assert!(!checkout.exists());
    let held: Vec<String> = std::fs::read_dir(setup.state.path().join("sluice/ports"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with('.'))
        .collect();
    assert!(held.is_empty(), "blocks of ports still held: {held:?}");
}
