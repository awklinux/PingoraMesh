use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use pingorahub_protocol::{ApiErrorBody, ApiResponse};
use serde::Serialize;
use serde_json::json;
use uuid::Uuid;

pub type ApiResult<T> = Result<Json<ApiResponse<T>>, AppError>;

#[derive(Debug, Serialize)]
pub struct ErrorEnvelope {
    pub request_id: Uuid,
    pub error: ApiErrorBody,
}

#[derive(Debug, Clone)]
pub struct AppError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
}

impl AppError {
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "bad_request",
            message: message.into(),
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: "not_found",
            message: message.into(),
        }
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            code: "conflict",
            message: message.into(),
        }
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized",
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "internal_error",
            message: message.into(),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let body = ErrorEnvelope {
            request_id: Uuid::new_v4(),
            error: ApiErrorBody {
                code: self.code.to_string(),
                message: self.message,
                details: json!({}),
            },
        };
        (self.status, Json(body)).into_response()
    }
}
