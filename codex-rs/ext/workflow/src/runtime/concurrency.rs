use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::sync::Arc;

use tokio::sync::OwnedSemaphorePermit;
use tokio::sync::RwLock;
use tokio::sync::Semaphore;

/// Hierarchical run/branch concurrency limiter.
#[derive(Clone)]
pub struct ConcurrencyLimiter {
    run: Arc<Semaphore>,
    branches: Arc<RwLock<BTreeMap<String, Arc<Semaphore>>>>,
}

impl ConcurrencyLimiter {
    pub fn new(run_limit: NonZeroUsize) -> Self {
        Self {
            run: Arc::new(Semaphore::new(run_limit.get())),
            branches: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }

    pub async fn acquire(
        &self,
        branch: Option<(&str, NonZeroUsize)>,
    ) -> Result<NodePermit, ConcurrencyError> {
        let run = Arc::clone(&self.run)
            .acquire_owned()
            .await
            .map_err(|_| ConcurrencyError::Closed)?;
        let branch = if let Some((key, limit)) = branch {
            let semaphore = {
                let mut branches = self.branches.write().await;
                Arc::clone(
                    branches
                        .entry(key.to_string())
                        .or_insert_with(|| Arc::new(Semaphore::new(limit.get()))),
                )
            };
            Some(
                semaphore
                    .acquire_owned()
                    .await
                    .map_err(|_| ConcurrencyError::Closed)?,
            )
        } else {
            None
        };
        Ok(NodePermit {
            _run: run,
            _branch: branch,
        })
    }
}

/// RAII permit releasing both branch and run capacity.
pub struct NodePermit {
    _run: OwnedSemaphorePermit,
    _branch: Option<OwnedSemaphorePermit>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ConcurrencyError {
    #[error("workflow concurrency limiter is closed")]
    Closed,
}
