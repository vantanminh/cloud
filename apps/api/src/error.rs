use std::collections::BTreeMap;

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

#[derive(Debug)]
pub enum AppError {
    BadRequest {
        code: &'static str,
        message: &'static str,
    },
    Unauthorized {
        code: &'static str,
        message: &'static str,
    },
    Forbidden {
        code: &'static str,
        message: &'static str,
    },
    NotFound {
        code: &'static str,
        message: &'static str,
    },
    Conflict {
        code: &'static str,
        message: &'static str,
    },
    ServiceUnavailable {
        code: &'static str,
        message: &'static str,
    },
    Validation {
        fields: BTreeMap<String, String>,
    },
    Internal(String),
}

#[derive(Debug, Serialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    code: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    fields: Option<BTreeMap<String, String>>,
}

impl AppError {
    pub fn validation(fields: BTreeMap<String, String>) -> Self {
        Self::Validation { fields }
    }

    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::NotFound { .. })
    }

    pub fn internal(error: impl std::fmt::Display) -> Self {
        Self::Internal(error.to_string())
    }
}

impl From<sqlx::Error> for AppError {
    fn from(error: sqlx::Error) -> Self {
        Self::internal(error)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            Self::BadRequest { code, message } => (
                StatusCode::BAD_REQUEST,
                ErrorBody {
                    code: code.to_owned(),
                    message: message.to_owned(),
                    fields: None,
                },
            ),
            Self::Unauthorized { code, message } => (
                StatusCode::UNAUTHORIZED,
                ErrorBody {
                    code: code.to_owned(),
                    message: message.to_owned(),
                    fields: None,
                },
            ),
            Self::Forbidden { code, message } => (
                StatusCode::FORBIDDEN,
                ErrorBody {
                    code: code.to_owned(),
                    message: message.to_owned(),
                    fields: None,
                },
            ),
            Self::NotFound { code, message } => (
                StatusCode::NOT_FOUND,
                ErrorBody {
                    code: code.to_owned(),
                    message: message.to_owned(),
                    fields: None,
                },
            ),
            Self::Conflict { code, message } => (
                StatusCode::CONFLICT,
                ErrorBody {
                    code: code.to_owned(),
                    message: message.to_owned(),
                    fields: None,
                },
            ),
            Self::ServiceUnavailable { code, message } => (
                StatusCode::SERVICE_UNAVAILABLE,
                ErrorBody {
                    code: code.to_owned(),
                    message: message.to_owned(),
                    fields: None,
                },
            ),
            Self::Validation { fields } => (
                StatusCode::UNPROCESSABLE_ENTITY,
                ErrorBody {
                    code: "VALIDATION_ERROR".to_owned(),
                    message: "Please check the highlighted fields.".to_owned(),
                    fields: Some(fields),
                },
            ),
            Self::Internal(error) => {
                tracing::error!(error = %error, "internal server error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ErrorBody {
                        code: "INTERNAL_ERROR".to_owned(),
                        message: "Something went wrong. Please try again.".to_owned(),
                        fields: None,
                    },
                )
            }
        };

        (status, Json(ErrorEnvelope { error: body })).into_response()
    }
}
