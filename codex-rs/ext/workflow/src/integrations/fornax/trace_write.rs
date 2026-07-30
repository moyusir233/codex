use std::collections::BTreeMap;
use std::sync::atomic::AtomicBool;

use serde_json::Value;
use sha2::Digest;
use sha2::Sha256;
use uuid::Uuid;

use super::FORNAX_BRIDGE_PROTOCOL;
use super::FORNAX_BRIDGE_VERSION;
use super::FORNAX_SDK_VERSION;
use super::FinishSpanRequest;
use super::FinishedSpan;
use super::FornaxBridgeConfig;
use super::FornaxBridgeError;
use super::FornaxBridgeHttpClient;
use super::RecordSpanRequest;
use super::RecordedSpan;
use super::SpanParent;
use super::SpanRecord;
use super::SpanType;
use super::StartSpanRequest;
use super::StartedSpan;
use super::ensure_bridge;

#[derive(Clone, Debug)]
pub struct FornaxTraceWriter {
    client: FornaxBridgeHttpClient,
}

impl FornaxTraceWriter {
    fn new(client: FornaxBridgeHttpClient) -> Self {
        Self { client }
    }

    /// Performs every local compatibility check and the recorded live-delivery gate.
    pub async fn preflight(
        config: &FornaxBridgeConfig,
        cancelled: &AtomicBool,
        live_delivery_approved: bool,
    ) -> Result<Self, FornaxBridgeError> {
        if !live_delivery_approved {
            return Err(FornaxBridgeError::LiveDeliveryNotApproved);
        }
        let discovery = ensure_bridge(config, cancelled)?;
        let client = FornaxBridgeHttpClient::new(
            &discovery,
            &config.state_dir,
            config.connect_timeout,
            config.request_timeout,
        )?;
        let health = client.health().await?;
        if health.protocol_version != FORNAX_BRIDGE_PROTOCOL
            || health.bridge_version != FORNAX_BRIDGE_VERSION
            || health.sdk_version != FORNAX_SDK_VERSION
            || health.instance_id != discovery.instance_id
        {
            return Err(FornaxBridgeError::VersionMismatch);
        }
        Ok(Self::new(client))
    }

    pub async fn start(
        &self,
        operation_id: Uuid,
        name: impl Into<String>,
        span_type: SpanType,
        parent: SpanParent,
    ) -> Result<StartedSpan, FornaxBridgeError> {
        self.client
            .start_span(&StartSpanRequest {
                operation_id,
                name: name.into(),
                span_type,
                parent,
            })
            .await
    }

    pub async fn record(
        &self,
        span_handle_id: Uuid,
        operation_id: Uuid,
        record: SpanRecord,
    ) -> Result<RecordedSpan, FornaxBridgeError> {
        self.client
            .record_span(
                span_handle_id,
                &RecordSpanRequest {
                    operation_id,
                    record,
                },
            )
            .await
    }

    pub async fn finish(
        &self,
        span_handle_id: Uuid,
        operation_id: Uuid,
    ) -> Result<FinishedSpan, FornaxBridgeError> {
        self.client
            .finish_span(span_handle_id, &FinishSpanRequest { operation_id })
            .await
    }
}

pub fn digest_record(bytes: &[u8], classification: &str) -> SpanRecord {
    let mut value = serde_json::Map::new();
    value.insert("type".to_string(), Value::String("digest".to_string()));
    value.insert(
        "sha256".to_string(),
        Value::String(format!("{:x}", Sha256::digest(bytes))),
    );
    value.insert("byteCount".to_string(), Value::from(bytes.len() as u64));
    value.insert(
        "classification".to_string(),
        Value::String(classification.to_string()),
    );
    SpanRecord::Input {
        value: Value::Object(value),
    }
}

pub fn safe_tags(values: impl IntoIterator<Item = (String, Value)>) -> SpanRecord {
    SpanRecord::Tags {
        values: BTreeMap::from_iter(values),
    }
}
