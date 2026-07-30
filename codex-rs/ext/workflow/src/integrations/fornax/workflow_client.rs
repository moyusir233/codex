use super::DurableFornaxTraceWriter;
use super::FornaxCli;

/// Workflow-safe facade over verified Fornax read/save and optional trace-write facets.
#[derive(Clone)]
pub struct FornaxWorkflowClient {
    cli: FornaxCli,
    trace_writer: Option<DurableFornaxTraceWriter>,
}

impl FornaxWorkflowClient {
    pub fn new(cli: FornaxCli, trace_writer: Option<DurableFornaxTraceWriter>) -> Self {
        Self { cli, trace_writer }
    }

    pub fn prompts(&self) -> &FornaxCli {
        &self.cli
    }

    pub fn skills(&self) -> &FornaxCli {
        &self.cli
    }

    pub fn trace_reader(&self) -> &FornaxCli {
        &self.cli
    }

    pub fn trace_writer(&self) -> Result<&DurableFornaxTraceWriter, FornaxCapabilityError> {
        self.trace_writer
            .as_ref()
            .ok_or(FornaxCapabilityError::TraceWriteUnavailable)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FornaxCapabilityError {
    #[error("Fornax trace writing is not available for this workflow")]
    TraceWriteUnavailable,
}
