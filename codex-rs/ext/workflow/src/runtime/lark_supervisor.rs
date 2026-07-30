use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::sync::Notify;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

use crate::integrations::lark::LarkEvent;
use crate::integrations::lark::LarkEventDecoder;
use crate::integrations::lark::LarkSubscriberSpec;

use super::LarkInteractionService;

/// One process-scoped owner for the `lark-cli event +subscribe` lock.
pub struct LarkEventSupervisor {
    service: Arc<LarkInteractionService>,
    spec: LarkSubscriberSpec,
    decoder: LarkEventDecoder,
    reconnect_delay: Duration,
    events: broadcast::Sender<LarkEvent>,
    task: Mutex<Option<RunningSubscriber>>,
}

struct RunningSubscriber {
    stopping: Arc<AtomicBool>,
    stop: Arc<Notify>,
    handle: JoinHandle<()>,
}

impl LarkEventSupervisor {
    pub fn new(
        service: Arc<LarkInteractionService>,
        maximum_line_bytes: usize,
        maximum_text_bytes: usize,
        reconnect_delay: Duration,
    ) -> Self {
        let spec = LarkSubscriberSpec::from_cli(service.cli());
        let (events, _) = broadcast::channel(256);
        Self {
            service,
            spec,
            decoder: LarkEventDecoder::new(maximum_line_bytes, maximum_text_bytes),
            reconnect_delay,
            events,
            task: Mutex::new(None),
        }
    }

    /// Returns an independent receiver; dropping it never stops the shared child.
    pub fn subscribe(&self) -> broadcast::Receiver<LarkEvent> {
        self.events.subscribe()
    }

    /// Starts the singleton if absent. Concurrent callers elect exactly one owner.
    pub async fn ensure(&self) -> bool {
        let mut task = self.task.lock().await;
        if task
            .as_ref()
            .is_some_and(|running| !running.handle.is_finished())
        {
            return false;
        }
        let stopping = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(Notify::new());
        let handle = tokio::spawn(run_subscriber(
            Arc::clone(&self.service),
            self.spec.clone(),
            self.decoder.clone(),
            self.reconnect_delay,
            self.events.clone(),
            Arc::clone(&stopping),
            Arc::clone(&stop),
        ));
        *task = Some(RunningSubscriber {
            stopping,
            stop,
            handle,
        });
        true
    }

    pub async fn is_running(&self) -> bool {
        self.task
            .lock()
            .await
            .as_ref()
            .is_some_and(|running| !running.handle.is_finished())
    }

    /// Gracefully stops only the owned process and preserves all durable waits.
    pub async fn stop(&self) -> Result<(), LarkSupervisorError> {
        let Some(running) = self.task.lock().await.take() else {
            return Ok(());
        };
        running.stopping.store(true, Ordering::Release);
        running.stop.notify_waiters();
        running.handle.await?;
        Ok(())
    }
}

async fn run_subscriber(
    service: Arc<LarkInteractionService>,
    spec: LarkSubscriberSpec,
    decoder: LarkEventDecoder,
    reconnect_delay: Duration,
    events: broadcast::Sender<LarkEvent>,
    stopping: Arc<AtomicBool>,
    stop: Arc<Notify>,
) {
    while !stopping.load(Ordering::Acquire) {
        let child = Command::new(&spec.executable)
            .args(&spec.args)
            .current_dir(&spec.cwd)
            .env_clear()
            .envs(&spec.environment)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn();
        let mut child = match child {
            Ok(child) => child,
            Err(error) => {
                tracing::warn!("failed to start Lark event subscriber: {error}");
                if wait_or_stop(reconnect_delay, &stopping, &stop).await {
                    return;
                }
                continue;
            }
        };
        let Some(mut stdout) = child.stdout.take() else {
            let _ = child.kill().await;
            return;
        };
        let mut buffer = [0_u8; 4096];
        let mut line = Vec::new();
        let mut discarding = false;
        loop {
            let read = tokio::select! {
                _ = stop.notified() => {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    return;
                }
                read = stdout.read(&mut buffer) => read,
            };
            let read = match read {
                Ok(0) => break,
                Ok(read) => read,
                Err(error) => {
                    tracing::warn!("failed to read Lark event stream: {error}");
                    break;
                }
            };
            for byte in &buffer[..read] {
                if *byte == b'\n' {
                    if !discarding && !line.is_empty() {
                        match decoder.decode(&line) {
                            Ok(event) => {
                                if let Err(error) = service.ingest_event(&event, "lark.event").await
                                {
                                    tracing::warn!(
                                        event_id = event.event_id,
                                        "failed to persist Lark event: {error}"
                                    );
                                }
                                let _ = events.send(event);
                            }
                            Err(error) => {
                                tracing::warn!("discarded invalid Lark event: {error}");
                            }
                        }
                    }
                    line.clear();
                    discarding = false;
                } else if line.len() < decoder.maximum_line_bytes() {
                    if !discarding {
                        line.push(*byte);
                    }
                } else {
                    line.clear();
                    discarding = true;
                }
            }
        }
        let _ = child.wait().await;
        if wait_or_stop(reconnect_delay, &stopping, &stop).await {
            return;
        }
    }
}

async fn wait_or_stop(delay: Duration, stopping: &AtomicBool, stop: &Notify) -> bool {
    if stopping.load(Ordering::Acquire) {
        return true;
    }
    tokio::select! {
        _ = tokio::time::sleep(delay) => stopping.load(Ordering::Acquire),
        _ = stop.notified() => true,
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LarkSupervisorError {
    #[error("Lark event supervisor task failed: {0}")]
    Join(#[from] tokio::task::JoinError),
}
