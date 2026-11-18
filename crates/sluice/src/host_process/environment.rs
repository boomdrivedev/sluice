//! The environment sluice's host process starts with (135; design D3): the
//! host's own, never its starting command's place. It keeps what a run
//! keeps of the environment it is started from, the locale, where the
//! user keeps their settings and state, the agent the user's keys are in,
//! where telemetry and the container engine are, sluice's own settings,
//! and what sets where git's configuration comes from and whether it may
//! prompt. It keeps nothing else: no variable that names a repository,
//! worktree, index or object store, no multiplexer session's, and none of
//! what the check profile names as a development shell's own. A run is
//! equipped from its repository's policy (147).

use std::ffi::OsString;

use crate::profiles::checks::{run_keeps, SHELL_ONLY};

/// Kept by their whole name, beside what a run keeps.
const NAMED: [&str; 5] = [
    "SSH_AUTH_SOCK",
    "GIT_CONFIG_GLOBAL",
    "GIT_CONFIG_NOSYSTEM",
    "GIT_TERMINAL_PROMPT",
    "GCM_INTERACTIVE",
];

/// Kept by the start of their names.
const PREFIXED: [&str; 4] = ["XDG_", "OTEL_", "DOCKER_", "SLUICE_"];

/// The command's own agent, which names the session it runs in (51).
const AGENT: &str = "SLUICE_AGENT";

/// Of `vars`, the starting command's environment, what the host process
/// starts with, in their order.
pub fn kept(vars: impl IntoIterator<Item = (OsString, OsString)>) -> Vec<(OsString, OsString)> {
    vars.into_iter()
        .filter(|(k, _)| k.to_str().is_some_and(keeps))
        .collect()
}

/// Whether the host process keeps `name` of its starter's environment.
fn keeps(name: &str) -> bool {
    if name == AGENT || SHELL_ONLY.iter().any(|p| name.starts_with(p)) && !run_keeps(name) {
        return false;
    }
    run_keeps(name) || NAMED.contains(&name) || PREFIXED.iter().any(|p| name.starts_with(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(vars: &[(&str, &str)]) -> Vec<String> {
        kept(
            vars.iter()
                .map(|(k, v)| (OsString::from(k), OsString::from(v))),
        )
        .into_iter()
        .map(|(k, _)| k.into_string().unwrap())
        .collect()
    }

    /// 135 (design D3): nothing of the command that started it: its
    /// repository, worktree, index and object store, its multiplexer's
    /// session, its development shell and its agent.
    #[test]
    fn the_host_process_keeps_none_of_its_starters_place() {
        let mut started_from: Vec<(String, &str)> = [
            ("GIT_DIR", "/work/repo/.git"),
            ("GIT_WORK_TREE", "/work/repo"),
            ("GIT_INDEX_FILE", "/work/repo/.git/index"),
            ("GIT_COMMON_DIR", "/work/repo/.git"),
            ("GIT_OBJECT_DIRECTORY", "/work/repo/.git/objects"),
            ("GIT_ALTERNATE_OBJECT_DIRECTORIES", "/elsewhere"),
            ("GIT_PREFIX", "src/"),
            ("TMUX", "/tmp/tmux-501/default,1,0"),
            ("TMUX_PANE", "%1"),
            ("SLUICE_AGENT", "agent-1"),
            ("PWD", "/work/repo"),
            ("OLDPWD", "/work"),
            ("GIT_ASKPASS", "/bin/askpass"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        // A multiplexer's pane's session, as the driver profile names it.
        for pane in crate::profiles::session_variables() {
            started_from.push((pane.to_string(), "p-1"));
        }
        // What the check profile names as a development shell's own.
        for shell in SHELL_ONLY {
            started_from.push((format!("{shell}ROOT"), "/work/repo"));
        }
        let vars: Vec<(&str, &str)> = started_from.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        assert_eq!(of(&vars), Vec::<String>::new());
    }

    /// 135 (design D3): the host's own: what a run keeps, the locale,
    /// the user's settings and state, the agent of their keys, telemetry,
    /// the container engine, sluice's settings and git's configuration.
    #[test]
    fn the_host_process_keeps_the_hosts_own() {
        let host = [
            ("PATH", "/usr/bin:/bin"),
            ("HOME", "/Users/u"),
            ("USER", "u"),
            ("LOGNAME", "u"),
            ("SHELL", "/bin/zsh"),
            ("TMPDIR", "/var/folders/x"),
            ("LANG", "en_US.UTF-8"),
            ("TERM", "xterm"),
            ("LC_ALL", "C"),
            ("XDG_STATE_HOME", "/Users/u/.local/state"),
            ("XDG_CONFIG_HOME", "/Users/u/.config"),
            ("SSH_AUTH_SOCK", "/tmp/agent.sock"),
            ("OTEL_EXPORTER_OTLP_ENDPOINT", "http://localhost:4318"),
            ("DOCKER_HOST", "unix:///var/run/docker.sock"),
            ("SLUICE_ROUND_SLEEP", "record"),
            ("SLUICE_USER_SCRATCH", "/tmp/s"),
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_TERMINAL_PROMPT", "0"),
            ("GCM_INTERACTIVE", "never"),
        ];
        let names: Vec<&str> = host.iter().map(|(k, _)| *k).collect();
        assert_eq!(of(&host), names);
        // What a run keeps of the environment it starts from, the tool
        // homes on `PATH` among it, is kept whatever the shell names.
        for (k, _) in std::env::vars().filter(|(k, _)| run_keeps(k)) {
            assert_eq!(of(&[(k.as_str(), "x")]), std::slice::from_ref(&k));
        }
    }
}
