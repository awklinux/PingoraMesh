use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub request_id: Uuid,
    pub data: T,
    pub meta: Value,
}

impl<T> ApiResponse<T> {
    pub fn ok(request_id: Uuid, data: T) -> Self {
        Self {
            request_id,
            data,
            meta: Value::Object(Default::default()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiErrorBody {
    pub code: String,
    pub message: String,
    pub details: Value,
}
