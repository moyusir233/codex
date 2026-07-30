use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Mutex;

use crate::WorkflowCancellation;
use crate::WorkflowRunId;

/// Process-local fan-out for cooperative cancellation of active effects.
#[derive(Clone, Default)]
pub(crate) struct CancellationSignals {
    signals: Arc<Mutex<BTreeMap<String, WorkflowCancellation>>>,
}

impl CancellationSignals {
    pub(crate) fn signal(&self, run_id: WorkflowRunId, requested: bool) -> WorkflowCancellation {
        let mut signals = self
            .signals
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let signal = signals
            .entry(run_id.to_string())
            .or_insert_with(|| WorkflowCancellation::new(requested))
            .clone();
        if requested {
            signal.request();
        }
        signal
    }

    pub(crate) fn request(&self, run_id: WorkflowRunId) {
        self.signal(run_id, true).request();
    }

    pub(crate) fn release(&self, run_id: WorkflowRunId) {
        self.signals
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&run_id.to_string());
    }
}
