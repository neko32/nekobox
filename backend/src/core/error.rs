use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("LM Studio error: {0}")]
    LmStudio(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("HTTP request error: {0}")]
    HttpRequest(#[from] reqwest::Error),

    #[error("Validation error: {0}")]
    Validation(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, id) = match &self {
            AppError::Database(_) => (StatusCode::INTERNAL_SERVER_ERROR, "DB_ERROR"),
            AppError::LmStudio(_) => (StatusCode::BAD_GATEWAY, "LM_STUDIO_ERROR"),
            AppError::Config(_) => (StatusCode::INTERNAL_SERVER_ERROR, "CONFIG_ERROR"),
            AppError::HttpRequest(_) => (StatusCode::BAD_GATEWAY, "HTTP_REQUEST_ERROR"),
            AppError::Validation(_) => (StatusCode::BAD_REQUEST, "VALIDATION_ERROR"),
        };
        let message = self.to_string();
        (
            status,
            Json(serde_json::json!({ "id": id, "message": message })),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status_of(err: AppError) -> StatusCode {
        err.into_response().status()
    }

    #[test]
    fn database_error_returns_500() {
        assert_eq!(
            status_of(AppError::Database(sqlx::Error::RowNotFound)),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn lm_studio_error_returns_502() {
        assert_eq!(
            status_of(AppError::LmStudio("fail".into())),
            StatusCode::BAD_GATEWAY
        );
    }

    #[test]
    fn config_error_returns_500() {
        assert_eq!(
            status_of(AppError::Config("cfg".into())),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn validation_error_returns_400() {
        assert_eq!(
            status_of(AppError::Validation("bad".into())),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn error_display_contains_message() {
        let err = AppError::LmStudio("接続失敗".into());
        assert!(err.to_string().contains("接続失敗"));
    }
}
