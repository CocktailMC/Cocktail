//! Stable identity carried from a control Transfer through all init progress events.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferContext {
    pub id: String,
    pub label: String,
}
impl TransferContext {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            label: label.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressEvent {
    #[serde(flatten)]
    pub transfer: TransferContext,
    pub received: u64,
    pub total: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
