use axum::{
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use pos_types::ApiErrorBody;
use thiserror::Error;

/// Every failing endpoint returns a JSON body: `{ "code": ..., "message": ... }`.
#[derive(Debug, Error)]
pub enum ApiError {
    #[error("{message}")]
    BadRequest { code: &'static str, message: String },
    #[error("{message}")]
    Unauthorized { code: &'static str, message: String },
    #[error("{message}")]
    Forbidden { code: &'static str, message: String },
    #[error("{message}")]
    NotFound { code: &'static str, message: String },
    #[error("{message}")]
    Conflict { code: &'static str, message: String },
    #[error("{message}")]
    Locked {
        code: &'static str,
        message: String,
        retry_after_secs: u64,
    },
    #[error("database error")]
    Database(#[from] sqlx::Error),
    #[error("internal error: {0}")]
    Internal(String),
}

impl ApiError {
    #[must_use]
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::BadRequest {
            code: "bad_request",
            message: message.into(),
        }
    }

    #[must_use]
    pub fn unauthorized(code: &'static str, message: impl Into<String>) -> Self {
        Self::Unauthorized {
            code,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::Forbidden {
            code: "forbidden",
            message: message.into(),
        }
    }

    #[must_use]
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound {
            code: "not_found",
            message: message.into(),
        }
    }

    #[must_use]
    pub fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self::Conflict {
            code,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn locked(message: impl Into<String>, retry_after_secs: u64) -> Self {
        Self::Locked {
            code: "locked_out",
            message: message.into(),
            retry_after_secs,
        }
    }

    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }

    fn parts(&self) -> (StatusCode, &str, &str) {
        match self {
            Self::BadRequest { code, message } => (StatusCode::BAD_REQUEST, code, message),
            Self::Unauthorized { code, message } => (StatusCode::UNAUTHORIZED, code, message),
            Self::Forbidden { code, message } => (StatusCode::FORBIDDEN, code, message),
            Self::NotFound { code, message } => (StatusCode::NOT_FOUND, code, message),
            Self::Conflict { code, message } => (StatusCode::CONFLICT, code, message),
            Self::Locked {
                code,
                message,
                retry_after_secs: _,
            } => (StatusCode::TOO_MANY_REQUESTS, code, message),
            Self::Database(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "The database could not complete that request.",
            ),
            Self::Internal(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Something went wrong on the server.",
            ),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if matches!(self, Self::Database(_) | Self::Internal(_)) {
            tracing::error!(error = %self, "request failed");
        }

        let (status, code, message) = self.parts();
        let code = code.to_owned();
        let message = message.to_owned();

        let mut response = (status, Json(ApiErrorBody { code, message })).into_response();
        if let Self::Locked {
            retry_after_secs, ..
        } = self
        {
            if let Ok(value) = retry_after_secs.to_string().parse() {
                response.headers_mut().insert(header::RETRY_AFTER, value);
            }
        }
        response
    }
}
