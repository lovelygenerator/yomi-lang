//! Isolated execution of scripts. Starts itself as a child process (`yomi run --auto` or `yomi test --json`) and limits
//! time, output and memory.
//!
//! The core has no time limit (only the limit on the number of steps per `run`), so stopping it is done on the parent
//! side.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

/// Limits of isolated execution.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Execution time. When exceeded, the child process is killed.
    pub timeout: Duration,
    /// The total size of scripts and input (bytes).
    pub max_input_bytes: usize,
    /// The size of each of standard output and standard error (bytes). The excess is discarded.
    pub max_output_bytes: usize,
    /// The memory of the child process (KiB). The virtual address space on unix, the commit limit on Windows.
    pub memory_kib: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            timeout: Duration::from_secs(5),
            max_input_bytes: 1 << 20,
            max_output_bytes: 1 << 20,
            memory_kib: 1 << 20,
        }
    }
}

/// The settings of an exploration (`yomi test --json`).
#[derive(Debug, Clone)]
pub struct TestArgs {
    /// The number of random seeds to explore (`0..seeds`).
    pub seeds: u64,
    /// The upper bound of the number of states to visit.
    pub max_states: usize,
    /// The upper bound of the number of instructions of one `run`.
    pub max_steps: Option<u64>,
    /// The data of events to try at `event.wait` (name and data; each becomes `--event name=data`).
    pub events: Vec<(String, String)>,
}

/// A request to run.
#[derive(Debug, Clone)]
pub struct Request {
    /// Modules (names and sources). The one named `main` is the entry file.
    pub modules: Vec<(String, String)>,
    /// What to feed to standard input (responses to choices and input).
    pub input: String,
    /// The random seed.
    pub seed: Option<u64>,
    /// The label to start from (the same as `yomi run --start`).
    pub start: Option<String>,
    /// Explore the branches (`yomi test --json`) instead of running with `--auto`. `input` and `seed` are not used.
    pub test: Option<TestArgs>,
}

/// The result of running.
#[derive(Debug, Clone, Default)]
pub struct Outcome {
    /// Standard output.
    pub stdout: String,
    /// Standard error.
    pub stderr: String,
    /// The exit code (`None` when it was killed for timing out or ended by a signal).
    pub exit_code: Option<i32>,
    /// Whether it was killed for timing out.
    pub timed_out: bool,
    /// Whether the output exceeded the limit and the rest was discarded.
    pub truncated: bool,
}

/// An error from validating a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invalid {
    /// The total of scripts and input is too large.
    TooLarge { limit: usize },
    /// A module name cannot be used.
    ModuleName(String),
    /// There is no entry file (`main`).
    NoEntry,
    /// Two modules have the same name.
    Duplicate(String),
}

/// Checks a module name (in the form `a::b`; each part is alphanumerics and `_`).
pub fn valid_module_name(name: &str) -> bool {
    !name.is_empty()
        && name.split("::").all(|part| {
            !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
}

/// Validates a request.
pub fn validate(request: &Request, limits: &Limits) -> Result<(), Invalid> {
    let total: usize =
        request.modules.iter().map(|(_, s)| s.len()).sum::<usize>() + request.input.len();
    if total > limits.max_input_bytes {
        return Err(Invalid::TooLarge {
            limit: limits.max_input_bytes,
        });
    }
    let mut seen = std::collections::BTreeSet::new();
    for (name, _) in &request.modules {
        if !valid_module_name(name) {
            return Err(Invalid::ModuleName(name.clone()));
        }
        if !seen.insert(name.as_str()) {
            return Err(Invalid::Duplicate(name.clone()));
        }
    }
    if !seen.contains("main") {
        return Err(Invalid::NoEntry);
    }
    Ok(())
}

/// The output read (up to the limit), and whether anything beyond the limit was discarded.
type Captured = std::sync::Arc<std::sync::Mutex<(Vec<u8>, bool)>>;

/// Reads output up to the limit. Anything beyond the limit is read and discarded (so the child process does not block).
///
/// What is read is put into `captured` right away. Even if cut off by a timeout before finishing, the output so far can be
/// returned.
async fn read_capped<R: tokio::io::AsyncRead + Unpin>(
    mut reader: R,
    max: usize,
    captured: Captured,
) {
    let mut buf = [0u8; 8192];
    loop {
        match reader.read(&mut buf).await {
            Ok(0) | Err(_) => return,
            Ok(n) => {
                let mut guard = captured.lock().unwrap_or_else(|e| e.into_inner());
                let room = max.saturating_sub(guard.0.len());
                if n > room {
                    guard.1 = true;
                }
                guard.0.extend_from_slice(&buf[..n.min(room)]);
            }
        }
    }
}

/// After killing, even if grandchild processes keep holding the output pipes, give up after waiting this long.
const DRAIN_GRACE: Duration = Duration::from_millis(500);

/// Runs scripts in isolation. Validate the request first with [`validate`].
pub async fn run(exe: &Path, request: &Request, limits: &Limits) -> std::io::Result<Outcome> {
    let dir = tempfile::tempdir()?;
    for (name, source) in &request.modules {
        let mut path = dir.path().to_path_buf();
        for part in name.split("::") {
            path.push(part);
        }
        path.set_extension("yomi");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, source)?;
    }

    let mut cmd = Command::new(exe);
    cmd.env_clear();
    match &request.test {
        None => {
            cmd.arg("run").arg("main.yomi").arg("--auto");
        }
        Some(t) => {
            cmd.arg("test")
                .arg("main.yomi")
                .arg("--json")
                .arg("--seeds")
                .arg(t.seeds.to_string())
                .arg("--max-states")
                .arg(t.max_states.to_string());
            if let Some(steps) = t.max_steps {
                cmd.arg("--max-steps").arg(steps.to_string());
            }
            for (name, data) in &t.events {
                cmd.arg("--event").arg(format!("{name}={data}"));
            }
        }
    }
    cmd.arg("--root").arg(".");
    if request.test.is_none()
        && let Some(seed) = request.seed
    {
        cmd.arg("--seed").arg(seed.to_string());
    }
    if let Some(start) = &request.start {
        cmd.arg("--start").arg(start);
    }
    cmd.current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let (mut child, guard) = job_limits::Limits::new()
        .process_memory(limits.memory_kib.saturating_mul(1024))
        .spawn_tokio(&mut cmd)?;

    let mut stdin = child.stdin.take();
    let input = request.input.clone().into_bytes();
    let feed = tokio::spawn(async move {
        if let Some(stdin) = stdin.as_mut() {
            // Ignore write failures when the child has already exited and closed
            let _ = stdin.write_all(&input).await;
            let _ = stdin.shutdown().await;
        }
    });
    let max = limits.max_output_bytes;
    let out_buf = Captured::default();
    let err_buf = Captured::default();
    let mut readers = Vec::new();
    if let Some(r) = child.stdout.take() {
        readers.push(tokio::spawn(read_capped(r, max, out_buf.clone())));
    }
    if let Some(r) = child.stderr.take() {
        readers.push(tokio::spawn(read_capped(r, max, err_buf.clone())));
    }

    let mut outcome = Outcome::default();
    match tokio::time::timeout(limits.timeout, child.wait()).await {
        Ok(status) => outcome.exit_code = status?.code(),
        Err(_) => {
            outcome.timed_out = true;
            let _ = child.kill().await;
        }
    }
    // Clean up before finishing reading, so grandchild processes do not keep holding the output pipes
    drop(guard);
    feed.abort();
    for reader in readers {
        let abort = reader.abort_handle();
        if tokio::time::timeout(DRAIN_GRACE, reader).await.is_err() {
            abort.abort();
        }
    }
    let take = |c: &Captured| {
        let guard = c.lock().unwrap_or_else(|e| e.into_inner());
        (String::from_utf8_lossy(&guard.0).into_owned(), guard.1)
    };
    let (out, out_cut) = take(&out_buf);
    let (err, err_cut) = take(&err_buf);
    outcome.stdout = out;
    outcome.stderr = err;
    outcome.truncated = out_cut || err_cut;
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(modules: &[(&str, &str)]) -> Request {
        Request {
            modules: modules
                .iter()
                .map(|(n, s)| (n.to_string(), s.to_string()))
                .collect(),
            input: String::new(),
            seed: None,
            start: None,
            test: None,
        }
    }

    #[test]
    fn module_names_must_be_identifiers_separated_by_double_colon() {
        for ok in ["main", "story::ch1", "a_1"] {
            assert!(valid_module_name(ok), "{ok}");
        }
        for bad in ["", "..", "a/b", "a::", "::a", "a b", "../x", "日本"] {
            assert!(!valid_module_name(bad), "{bad}");
        }
    }

    #[test]
    fn request_validation_checks_size_names_duplicates_and_entry_file() {
        let limits = Limits {
            max_input_bytes: 10,
            ..Limits::default()
        };
        assert_eq!(validate(&request(&[("main", "x")]), &limits), Ok(()));
        assert_eq!(
            validate(&request(&[("main", "0123456789ab")]), &limits),
            Err(Invalid::TooLarge { limit: 10 })
        );
        assert_eq!(
            validate(&request(&[("main", ""), ("../x", "")]), &limits),
            Err(Invalid::ModuleName("../x".into()))
        );
        assert_eq!(
            validate(&request(&[("main", ""), ("main", "")]), &limits),
            Err(Invalid::Duplicate("main".into()))
        );
        assert_eq!(
            validate(&request(&[("other", "")]), &limits),
            Err(Invalid::NoEntry)
        );
    }

    #[tokio::test]
    async fn output_over_limit_is_truncated_without_blocking_child() {
        let data = vec![b'a'; 100_000];
        let captured = Captured::default();
        read_capped(&data[..], 1000, captured.clone()).await;
        let guard = captured.lock().unwrap();
        assert!(guard.1);
        assert_eq!(guard.0.len(), 1000);
    }
}

#[cfg(all(test, unix))]
mod unix_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// Makes an sh script that ignores its arguments and runs `body`.
    fn fake_exe(dir: &Path, body: &str) -> std::path::PathBuf {
        let path = dir.join("fake-yomi");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn main_only() -> Request {
        Request {
            modules: vec![("main".into(), String::new())],
            input: "abc\n".into(),
            seed: None,
            start: None,
            test: None,
        }
    }

    #[tokio::test]
    async fn child_process_over_time_is_killed() {
        let dir = tempfile::tempdir().unwrap();
        let exe = fake_exe(dir.path(), "/bin/sleep 30");
        let limits = Limits {
            timeout: Duration::from_millis(300),
            ..Limits::default()
        };
        let started = std::time::Instant::now();
        let out = run(&exe, &main_only(), &limits).await.unwrap();
        assert!(out.timed_out);
        assert_eq!(out.exit_code, None);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[tokio::test]
    async fn output_is_cut_at_limit_and_stdin_and_exit_code_are_passed() {
        let dir = tempfile::tempdir().unwrap();
        let exe = fake_exe(
            dir.path(),
            "read line; echo \"got:$line\"; /usr/bin/head -c 100000 /dev/zero | /usr/bin/tr '\\0' a; exit 3",
        );
        let limits = Limits {
            max_output_bytes: 1000,
            ..Limits::default()
        };
        let out = run(&exe, &main_only(), &limits).await.unwrap();
        assert!(out.stdout.starts_with("got:abc\n"), "{:?}", out.stdout);
        assert_eq!(out.stdout.len(), 1000);
        assert!(out.truncated);
        assert_eq!(out.exit_code, Some(3));
        assert!(!out.timed_out);
    }

    #[tokio::test]
    async fn child_environment_is_empty_and_working_directory_is_scripts_temp_dir() {
        let dir = tempfile::tempdir().unwrap();
        let exe = fake_exe(dir.path(), "/usr/bin/env | /usr/bin/wc -l; ls main.yomi");
        let out = run(&exe, &main_only(), &Limits::default()).await.unwrap();
        let mut lines = out.stdout.lines();
        let env_count: u32 = lines.next().unwrap().trim().parse().unwrap();
        // Only what sh adds, such as PWD (the parent's environment variables are not passed)
        assert!(env_count <= 4, "{env_count}");
        assert_eq!(lines.next(), Some("main.yomi"));
    }
}
