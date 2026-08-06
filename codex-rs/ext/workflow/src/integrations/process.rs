use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Read;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Child;
use std::process::Command;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use super::fornax::redaction::Redactor;

/// One bounded, structured process invocation.
#[derive(Clone, Debug)]
pub struct ProcessRequest {
    /// Absolute executable path.
    pub executable: PathBuf,
    /// Structured argument vector.
    pub args: Vec<OsString>,
    /// Absolute controlled working directory.
    pub cwd: PathBuf,
    /// Complete environment exposed to the child.
    pub env: BTreeMap<OsString, OsString>,
    /// Values that must not occur in diagnostics.
    pub redactions: Vec<String>,
}

/// Successful bounded process output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessOutput {
    /// Child exit code.
    pub status_code: i32,
    /// Captured stdout. Callers must treat it as capability data, not a diagnostic.
    pub stdout: Vec<u8>,
    /// Bounded, redacted stderr diagnostics.
    pub stderr_diagnostic: String,
}

/// Failures from the production structured process runner.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProcessError {
    /// The executable was not absolute.
    #[error("external executable must be an absolute path")]
    ExecutableIsNotAbsolute,
    /// The working directory was not absolute.
    #[error("external working directory must be an absolute path")]
    WorkingDirectoryIsNotAbsolute,
    /// The child could not be started.
    #[error("failed to start external command: {message}")]
    Spawn {
        /// Redacted OS error.
        message: String,
    },
    /// Child status or output could not be collected.
    #[error("failed to collect external command output: {message}")]
    Collect {
        /// Redacted OS error.
        message: String,
    },
    /// The child returned a non-zero status.
    #[error("external command failed with exit {status_code}: {diagnostic}")]
    Exit {
        /// Process exit code.
        status_code: i32,
        /// Redacted stderr summary.
        diagnostic: String,
    },
    /// The deadline elapsed and the process was terminated.
    #[error("external command timed out")]
    Timeout,
    /// Cancellation was observed and the process was terminated.
    #[error("external command was cancelled")]
    Cancelled,
    /// A stream exceeded its configured bound.
    #[error("external command {stream} exceeded the {limit}-byte output limit")]
    OutputLimit {
        /// Stream whose bound was exceeded.
        stream: &'static str,
        /// Configured byte limit.
        limit: usize,
    },
}

/// Runs external commands with controlled environment, output, and lifetime.
#[derive(Clone, Debug)]
pub struct ProcessRunner {
    timeout: Duration,
    poll_interval: Duration,
    stdout_limit: usize,
    stderr_limit: usize,
}

impl ProcessRunner {
    /// Creates a production runner with explicit hard bounds.
    pub fn new(timeout: Duration, stdout_limit: usize, stderr_limit: usize) -> Self {
        Self {
            timeout,
            poll_interval: Duration::from_millis(5),
            stdout_limit,
            stderr_limit,
        }
    }

    /// Executes one structured request and cooperatively observes cancellation.
    pub fn run(
        &self,
        request: ProcessRequest,
        cancelled: &AtomicBool,
    ) -> Result<ProcessOutput, ProcessError> {
        if !request.executable.is_absolute() {
            return Err(ProcessError::ExecutableIsNotAbsolute);
        }
        if !request.cwd.is_absolute() {
            return Err(ProcessError::WorkingDirectoryIsNotAbsolute);
        }
        let redactor = Arc::new(Redactor::new(request.redactions));
        let mut command = Command::new(&request.executable);
        command
            .args(&request.args)
            .current_dir(&request.cwd)
            .env_clear()
            .envs(&request.env)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command.spawn().map_err(|error| ProcessError::Spawn {
            message: redactor.redact(&error.to_string()),
        })?;
        let stdout = spawn_reader(
            child.stdout.take().ok_or_else(|| ProcessError::Collect {
                message: "stdout pipe was unavailable".to_string(),
            })?,
            self.stdout_limit,
        );
        let stderr = spawn_reader(
            child.stderr.take().ok_or_else(|| ProcessError::Collect {
                message: "stderr pipe was unavailable".to_string(),
            })?,
            self.stderr_limit,
        );
        let started_at = Instant::now();
        let status = loop {
            if cancelled.load(Ordering::Acquire) {
                terminate(&mut child);
                join_reader(stdout, "stdout")?;
                join_reader(stderr, "stderr")?;
                return Err(ProcessError::Cancelled);
            }
            if started_at.elapsed() >= self.timeout {
                terminate(&mut child);
                join_reader(stdout, "stdout")?;
                join_reader(stderr, "stderr")?;
                return Err(ProcessError::Timeout);
            }
            match child.try_wait().map_err(|error| ProcessError::Collect {
                message: redactor.redact(&error.to_string()),
            })? {
                Some(status) => break status,
                None => thread::sleep(self.poll_interval),
            }
        };

        let stdout = join_reader(stdout, "stdout")?;
        let stderr = join_reader(stderr, "stderr")?;
        if stdout.exceeded {
            return Err(ProcessError::OutputLimit {
                stream: "stdout",
                limit: self.stdout_limit,
            });
        }
        if stderr.exceeded {
            return Err(ProcessError::OutputLimit {
                stream: "stderr",
                limit: self.stderr_limit,
            });
        }
        let stderr_diagnostic = redactor.redact(&String::from_utf8_lossy(&stderr.bytes));
        let status_code = status.code().unwrap_or(-1);
        if !status.success() {
            return Err(ProcessError::Exit {
                status_code,
                diagnostic: bounded_line(&stderr_diagnostic),
            });
        }
        Ok(ProcessOutput {
            status_code,
            stdout: stdout.bytes,
            stderr_diagnostic,
        })
    }
}

struct BoundedRead {
    bytes: Vec<u8>,
    exceeded: bool,
}

fn spawn_reader(
    mut pipe: impl Read + Send + 'static,
    limit: usize,
) -> thread::JoinHandle<std::io::Result<BoundedRead>> {
    thread::spawn(move || {
        let mut bytes = Vec::with_capacity(limit.min(8192));
        let mut exceeded = false;
        let mut buffer = [0_u8; 8192];
        loop {
            let read = pipe.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            let remaining = limit.saturating_sub(bytes.len());
            bytes.extend_from_slice(&buffer[..read.min(remaining)]);
            exceeded |= read > remaining;
        }
        Ok(BoundedRead { bytes, exceeded })
    })
}

fn join_reader(
    reader: thread::JoinHandle<std::io::Result<BoundedRead>>,
    stream: &'static str,
) -> Result<BoundedRead, ProcessError> {
    reader
        .join()
        .map_err(|_| ProcessError::Collect {
            message: format!("{stream} reader panicked"),
        })?
        .map_err(|error| ProcessError::Collect {
            message: format!("{stream}: {error}"),
        })
}

fn terminate(child: &mut Child) {
    #[cfg(unix)]
    {
        let process_group = i32::try_from(child.id()).unwrap_or(i32::MAX);
        // SAFETY: `process_group` is the positive id returned for the child we
        // spawned as its own process-group leader. A negative pid targets only
        // that group, and SIGKILL requires no shared Rust memory invariants.
        let _ = unsafe { libc::kill(-process_group, libc::SIGKILL) };
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn bounded_line(diagnostic: &str) -> String {
    diagnostic
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("no diagnostic was returned")
        .chars()
        .take(512)
        .collect()
}
