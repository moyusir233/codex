use super::LarkCliCapabilities;

/// Exact live-safety decision for the pinned CLI profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LarkWorkflowCompatibility {
    Supported,
    MissingRevisionAwareDocumentUpdates,
}

impl LarkCliCapabilities {
    /// Returns whether this profile can safely perform the feature workflow.
    pub fn lark_feature_workflow_compatibility(self) -> LarkWorkflowCompatibility {
        if self.documents
            && self.document_revisions
            && self.document_revision_aware_updates
            && self.chats
            && self.identity_search
            && self.chat_members
            && self.idempotent_messages
            && self.raw_events
        {
            LarkWorkflowCompatibility::Supported
        } else {
            LarkWorkflowCompatibility::MissingRevisionAwareDocumentUpdates
        }
    }
}
