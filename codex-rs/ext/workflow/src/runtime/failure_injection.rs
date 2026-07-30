use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Mutex;

/// Deterministic crash boundary used only by workflow failure tests.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FailurePoint {
    BeforeDbWrite,
    AfterDbWrite,
    BeforeThreadCreate,
    AfterThreadCreate,
    BeforeTurnQueue,
    AfterTurnQueue,
    AfterUserBoundaryAppend,
    AfterTerminalDelivery,
    AfterTerminalFlush,
    BeforeExternalDispatch,
    AfterExternalDispatch,
    BeforeReplyCommit,
    AfterReplyCommit,
}

#[derive(Clone, Default)]
pub struct FailureInjector {
    inner: Arc<Mutex<FailureInjectorState>>,
}

#[derive(Default)]
struct FailureInjectorState {
    target: Option<(FailurePoint, u32)>,
    visits: BTreeMap<FailurePoint, u32>,
}

impl FailureInjector {
    pub fn disabled() -> Self {
        Self::default()
    }

    pub fn fail_on(point: FailurePoint, visit: u32) -> Self {
        Self {
            inner: Arc::new(Mutex::new(FailureInjectorState {
                target: Some((point, visit.max(1))),
                visits: BTreeMap::new(),
            })),
        }
    }

    pub fn checkpoint(&self, point: FailurePoint) -> Result<(), InjectedFailure> {
        let mut state = self.inner.lock().map_err(|_| InjectedFailure { point })?;
        let visit = state.visits.entry(point).or_default();
        *visit = visit.saturating_add(1);
        let current_visit = *visit;
        if state.target == Some((point, current_visit)) {
            return Err(InjectedFailure { point });
        }
        Ok(())
    }

    pub fn visits(&self, point: FailurePoint) -> u32 {
        self.inner
            .lock()
            .ok()
            .and_then(|state| state.visits.get(&point).copied())
            .unwrap_or(0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("injected workflow failure at {point:?}")]
pub struct InjectedFailure {
    pub point: FailurePoint,
}
