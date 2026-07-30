use std::ffi::OsString;
use std::io::Read;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

/// Output captured from an experimental external capability probe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessOutput {
    /// Child exit status, or `None` when terminated before normal exit.
    pub status_code: Option<i32>,
    /// Captured standard output.
    pub stdout: Vec<u8>,
    /// Captured standard error.
    pub stderr: Vec<u8>,
}

/// Failure modes distinguished by the experimental process harness.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProcessFailure {
    /// The executable path was relative.
    ExecutableIsNotAbsolute,
    /// The child could not be spawned.
    Spawn(String),
    /// Child status or output could not be collected.
    Wait(String),
    /// The deadline elapsed and the child was terminated.
    Timeout(ProcessOutput),
    /// Cancellation was observed and the child was terminated.
    Cancelled(ProcessOutput),
}

/// Minimal structured process harness used only by workflow feasibility tests.
///
/// It accepts an absolute executable plus individual arguments, clears the
/// inherited environment, supports cooperative cancellation, and terminates a
/// child when the configured deadline expires. The production bounded-output
/// runner remains a later milestone.
#[derive(Clone, Debug)]
pub struct FeasibilityProcessRunner {
    timeout: Duration,
    poll_interval: Duration,
}

impl FeasibilityProcessRunner {
    /// Creates a runner with the supplied hard deadline.
    pub fn new(timeout: Duration) -> Self {
        Self {
            timeout,
            poll_interval: Duration::from_millis(5),
        }
    }

    /// Runs one command with structured arguments and a controlled directory.
    pub fn run(
        &self,
        executable: &Path,
        args: &[OsString],
        cwd: &Path,
        cancelled: &AtomicBool,
    ) -> Result<ProcessOutput, ProcessFailure> {
        if !executable.is_absolute() {
            return Err(ProcessFailure::ExecutableIsNotAbsolute);
        }

        let mut child = Command::new(executable)
            .args(args)
            .current_dir(cwd)
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| ProcessFailure::Spawn(err.to_string()))?;
        let started_at = Instant::now();

        loop {
            if cancelled.load(Ordering::Acquire) {
                terminate(&mut child);
                return Err(ProcessFailure::Cancelled(capture_output(child)?));
            }
            if started_at.elapsed() >= self.timeout {
                terminate(&mut child);
                return Err(ProcessFailure::Timeout(capture_output(child)?));
            }
            match child
                .try_wait()
                .map_err(|err| ProcessFailure::Wait(err.to_string()))?
            {
                Some(status) => {
                    let mut output = capture_pipes(&mut child)?;
                    output.status_code = status.code();
                    return Ok(output);
                }
                None => std::thread::sleep(self.poll_interval),
            }
        }
    }
}

impl std::fmt::Display for ProcessFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ExecutableIsNotAbsolute => formatter.write_str("executable path is not absolute"),
            Self::Spawn(message) => write!(formatter, "failed to spawn process: {message}"),
            Self::Wait(message) => write!(formatter, "failed to collect process output: {message}"),
            Self::Timeout(_) => formatter.write_str("process timed out"),
            Self::Cancelled(_) => formatter.write_str("process was cancelled"),
        }
    }
}

impl std::error::Error for ProcessFailure {}

fn terminate(child: &mut std::process::Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn capture_output(mut child: std::process::Child) -> Result<ProcessOutput, ProcessFailure> {
    capture_pipes(&mut child)
}

fn capture_pipes(child: &mut std::process::Child) -> Result<ProcessOutput, ProcessFailure> {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    if let Some(mut pipe) = child.stdout.take() {
        pipe.read_to_end(&mut stdout)
            .map_err(|err| ProcessFailure::Wait(err.to_string()))?;
    }
    if let Some(mut pipe) = child.stderr.take() {
        pipe.read_to_end(&mut stderr)
            .map_err(|err| ProcessFailure::Wait(err.to_string()))?;
    }
    Ok(ProcessOutput {
        status_code: None,
        stdout,
        stderr,
    })
}
