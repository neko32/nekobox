use axum::{
    extract::{Path, State},
    Json,
};
use serde::Serialize;
use std::sync::Arc;

use crate::{core::error::AppError, AppState};

// ───────────────────────────────────── Response ────────────────────────────

#[derive(Debug, Serialize)]
pub struct SessionLogEntryDto {
    pub msg_sender_name: String,
    pub user_name: String,
    pub msg: String,
    pub timestamp: String,
}

#[derive(Debug, Serialize)]
pub struct SessionHistoryResponse {
    pub session_id: String,
    pub entries: Vec<SessionLogEntryDto>,
}

// ───────────────────────────────────── ハンドラ ────────────────────────────

pub async fn sessions_handler(
    Path(session_id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<SessionHistoryResponse>, AppError> {
    let logs = state.db.get_logs_by_session(&session_id).await?;

    let entries = logs
        .into_iter()
        .map(|log| SessionLogEntryDto {
            msg_sender_name: log.msg_sender_name,
            user_name: log.user_name,
            msg: log.msg,
            timestamp: log.timestamp.to_rfc3339(),
        })
        .collect();

    Ok(Json(SessionHistoryResponse {
        session_id,
        entries,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{http::StatusCode, routing::get, Router};
    use axum_test::TestServer;
    use mockall::predicate::*;
    use std::sync::Arc;

    use crate::{
        api::lm_studio::MockLmStudioClient,
        core::{
            config::{AppConfig, CharacterConfig, ModelConfig},
            db::MockConversationRepository,
            models::SessionLog,
        },
        AppState,
    };

    fn make_config() -> AppConfig {
        AppConfig {
            current_session: "ses-001".to_string(),
            user_name: "さのまる".to_string(),
            background_image: None,
            character: CharacterConfig {
                name: "takochan".to_string(),
                version: "1.0.0".to_string(),
                model_path: None,
                settings_path: "/tmp".to_string(),
            },
            model: ModelConfig { temperature: 0.6 },
        }
    }

    fn make_server(db: MockConversationRepository) -> TestServer {
        let state = Arc::new(AppState {
            db: Arc::new(db),
            lm_client: Arc::new(MockLmStudioClient::new()),
            app_config: make_config(),
        });
        let app = Router::new()
            .route("/v1/sessions/{session_id}", get(sessions_handler))
            .with_state(state);
        TestServer::new(app)
    }

    fn sample_log(session_id: &str, sender: &str, user: &str, msg: &str) -> SessionLog {
        SessionLog {
            session_id: session_id.to_string(),
            session_alias: Some("test-alias".to_string()),
            background_image: "/bg.png".to_string(),
            msg_sender_name: sender.to_string(),
            user_name: user.to_string(),
            settings_name: "takochan_1.0.0".to_string(),
            msg: msg.to_string(),
            image_url: None,
            response_id: None,
            model_instance_id: None,
            input_tokens: None,
            total_output_tokens: None,
            timestamp: chrono::Utc::now(),
        }
    }

    #[tokio::test]
    async fn sessions_handler_returns_200_with_entries() {
        let mut db = MockConversationRepository::new();
        db.expect_get_logs_by_session()
            .with(eq("ses-001"))
            .once()
            .returning(|_| {
                Ok(vec![
                    sample_log("ses-001", "さのまる", "さのまる", "こんにちは"),
                    sample_log("ses-001", "takochan", "さのまる", "やあ！"),
                ])
            });

        let server = make_server(db);
        let res = server.get("/v1/sessions/ses-001").await;

        res.assert_status(StatusCode::OK);
        let json = res.json::<serde_json::Value>();
        assert_eq!(json["session_id"], "ses-001");
        assert_eq!(json["entries"].as_array().unwrap().len(), 2);
        assert_eq!(json["entries"][0]["msg"], "こんにちは");
        assert_eq!(json["entries"][1]["msg"], "やあ！");
    }

    #[tokio::test]
    async fn sessions_handler_returns_empty_for_no_logs() {
        let mut db = MockConversationRepository::new();
        db.expect_get_logs_by_session()
            .with(eq("ses-empty"))
            .once()
            .returning(|_| Ok(vec![]));

        let server = make_server(db);
        let res = server.get("/v1/sessions/ses-empty").await;

        res.assert_status(StatusCode::OK);
        let json = res.json::<serde_json::Value>();
        assert_eq!(json["entries"].as_array().unwrap().len(), 0);
    }
}
