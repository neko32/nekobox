use axum::{extract::State, Json};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{
    api::lm_studio::{ChatMessage, ChatRequest},
    core::{
        error::AppError,
        models::{Emotion, SessionLog},
    },
    AppState,
};

// ───────────────────────────────────── Request / Response ──────────────────

#[derive(Debug, Deserialize)]
pub struct MsgRequest {
    pub character_name: String,
    pub version: String,
    pub response_id: Option<String>,
    pub image_url: Option<String>,
    pub user_name: String,
    pub session_id: String,
    pub session_alias: Option<String>,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct MsgResponse {
    pub character_name: String,
    pub version: String,
    pub response_id: Option<String>,
    pub image_url: Option<String>,
    pub user_name: String,
    pub session_id: String,
    pub message: String,
    pub emotion: String,
}

/// LM Studio が JSON で返すと期待するレスポンス構造
#[derive(Debug, Deserialize)]
struct LmJsonContent {
    message: String,
    emotion: Option<String>,
}

// ───────────────────────────────────── ハンドラ ────────────────────────────

pub async fn msg_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<MsgRequest>,
) -> Result<Json<MsgResponse>, AppError> {
    // バリデーション
    if req.character_name.is_empty() {
        return Err(AppError::Validation("character_name is required".into()));
    }
    if req.user_name.is_empty() {
        return Err(AppError::Validation("user_name is required".into()));
    }
    if req.message.is_empty() {
        return Err(AppError::Validation("message is required".into()));
    }

    // system_prompt をキャラクター設定ファイルからロード
    let system_prompt = state.app_config.load_system_prompt()?;

    // LM Studio へ送るメッセージを構築
    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: system_prompt,
        },
        ChatMessage {
            role: "user".to_string(),
            content: req.message.clone(),
        },
    ];

    let lm_request = ChatRequest {
        model: state.app_config.character.name.clone(),
        messages,
        temperature: state.app_config.model.temperature,
    };

    // LM Studio にリクエスト送信
    let lm_response = state.lm_client.chat(lm_request).await?;

    let new_response_id = Some(lm_response.id.clone());
    let model_instance_id = lm_response.model.clone();
    let (input_tokens, output_tokens) = lm_response
        .usage
        .as_ref()
        .map_or((None, None), |u| (u.prompt_tokens, u.completion_tokens));

    let raw_content = lm_response
        .choices
        .first()
        .map(|c| c.message.content.clone())
        .unwrap_or_default();

    // LM Studio レスポンスから message と emotion を抽出
    let (character_message, emotion) = parse_lm_response(&raw_content);
    let settings_name = format!("{}_{}", req.character_name, req.version);
    let bg = state
        .app_config
        .background_image
        .clone()
        .unwrap_or_default();

    // ユーザーメッセージをDBに保存
    state
        .db
        .save_log(&SessionLog {
            session_id: req.session_id.clone(),
            session_alias: req.session_alias.clone(),
            background_image: bg.clone(),
            msg_sender_name: req.user_name.clone(),
            user_name: req.user_name.clone(),
            settings_name: settings_name.clone(),
            msg: req.message.clone(),
            image_url: req.image_url.clone(),
            response_id: req.response_id.clone(),
            model_instance_id: None,
            input_tokens: None,
            total_output_tokens: None,
            timestamp: Utc::now(),
        })
        .await?;

    // キャラクターのレスポンスをDBに保存
    state
        .db
        .save_log(&SessionLog {
            session_id: req.session_id.clone(),
            session_alias: req.session_alias.clone(),
            background_image: bg,
            msg_sender_name: req.character_name.clone(),
            user_name: req.user_name.clone(),
            settings_name,
            msg: character_message.clone(),
            image_url: None,
            response_id: new_response_id.clone(),
            model_instance_id,
            input_tokens,
            total_output_tokens: output_tokens,
            timestamp: Utc::now(),
        })
        .await?;

    Ok(Json(MsgResponse {
        character_name: req.character_name,
        version: req.version,
        response_id: new_response_id,
        image_url: None,
        user_name: req.user_name,
        session_id: req.session_id,
        message: character_message,
        emotion: emotion.as_str().to_string(),
    }))
}

/// LM Studio が返す JSON コンテンツをパースして (message, emotion) を返す
fn parse_lm_response(content: &str) -> (String, Emotion) {
    // LLM が JSON を ```json ... ``` で囲むことがあるため、コードブロックを除去する
    let stripped = strip_code_block(content);
    if let Ok(parsed) = serde_json::from_str::<LmJsonContent>(stripped) {
        let emotion = parsed
            .emotion
            .as_deref()
            .and_then(Emotion::from_str)
            .unwrap_or_default();
        return (parsed.message, emotion);
    }
    // JSON パース失敗時はそのままのテキストを返す
    (content.to_string(), Emotion::default())
}

/// ` ```json ... ``` ` または ` ``` ... ``` ` のコードブロックを除去して内側のテキストを返す
fn strip_code_block(s: &str) -> &str {
    let s = s.trim();
    let inner = s
        .strip_prefix("```json")
        .or_else(|| s.strip_prefix("```"))
        .and_then(|rest| rest.strip_suffix("```"))
        .map(str::trim);
    inner.unwrap_or(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{http::StatusCode, routing::post, Router};
    use axum_test::TestServer;
    use mockall::predicate::*;
    use std::sync::Arc;

    use crate::{
        api::lm_studio::{ChatChoice, ChatResponse, MockLmStudioClient},
        core::{
            config::{AppConfig, CharacterConfig, ModelConfig},
            db::MockConversationRepository,
        },
        AppState,
    };

    // ─── ヘルパー ─────────────────────────────────────────────

    /// テスト用の一時設定ファイルを作成して AppConfig を返す
    fn make_config() -> (AppConfig, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let prompt_md = "あなたはたこちゃんです。JSON形式で答えてください。";
        let prompt_file = tmp.path().join("takochan_1.0.0.md");
        std::fs::write(&prompt_file, prompt_md).unwrap();

        let cfg = AppConfig {
            current_session: "ses-001".to_string(),
            user_name: "さのまる".to_string(),
            background_image: Some("/bg.png".to_string()),
            character: CharacterConfig {
                name: "takochan".to_string(),
                version: "1.0.0".to_string(),
                model_path: None,
                settings_path: tmp.path().to_string_lossy().into_owned(),
            },
            model: ModelConfig { temperature: 0.6 },
        };
        (cfg, tmp) // tmp を返してドロップを防ぐ
    }

    fn lm_response(msg: &str, emotion: &str) -> ChatResponse {
        ChatResponse {
            id: "resp-001".to_string(),
            choices: vec![ChatChoice {
                message: crate::api::lm_studio::ChatMessage {
                    role: "assistant".to_string(),
                    content: format!(r#"{{"message":"{msg}","emotion":"{emotion}"}}"#),
                },
                finish_reason: Some("stop".to_string()),
            }],
            usage: None,
            model: Some("takochan".to_string()),
        }
    }

    fn make_server(
        lm: MockLmStudioClient,
        db: MockConversationRepository,
        config: AppConfig,
    ) -> TestServer {
        let state = Arc::new(AppState {
            db: Arc::new(db),
            lm_client: Arc::new(lm),
            app_config: config,
        });
        let app = Router::new()
            .route("/v1/msg", post(msg_handler))
            .with_state(state);
        TestServer::new(app)
    }

    // ─── parse_lm_response のユニットテスト ──────────────────

    #[test]
    fn parse_lm_response_valid_json() {
        let json = r#"{"message":"こんにちは！","emotion":"嬉しい"}"#;
        let (msg, emotion) = parse_lm_response(json);
        assert_eq!(msg, "こんにちは！");
        assert_eq!(emotion.as_str(), "嬉しい");
    }

    #[test]
    fn parse_lm_response_strips_json_code_block() {
        let wrapped = "```json\n{\"message\":\"にゃ！\",\"emotion\":\"楽しい\"}\n```";
        let (msg, emotion) = parse_lm_response(wrapped);
        assert_eq!(msg, "にゃ！");
        assert_eq!(emotion.as_str(), "楽しい");
    }

    #[test]
    fn parse_lm_response_strips_plain_code_block() {
        let wrapped = "```\n{\"message\":\"にゃ\",\"emotion\":\"普通\"}\n```";
        let (msg, emotion) = parse_lm_response(wrapped);
        assert_eq!(msg, "にゃ");
        assert_eq!(emotion.as_str(), "普通");
    }

    #[test]
    fn parse_lm_response_fallback_on_plain_text() {
        let plain = "こんにちは！";
        let (msg, emotion) = parse_lm_response(plain);
        assert_eq!(msg, "こんにちは！");
        assert_eq!(emotion.as_str(), "普通");
    }

    #[test]
    fn parse_lm_response_unknown_emotion_defaults_neutral() {
        let json = r#"{"message":"やあ","emotion":"不明な感情"}"#;
        let (_, emotion) = parse_lm_response(json);
        assert_eq!(emotion.as_str(), "普通");
    }

    #[test]
    fn parse_lm_response_missing_emotion_field_defaults_neutral() {
        let json = r#"{"message":"やあ"}"#;
        let (msg, emotion) = parse_lm_response(json);
        assert_eq!(msg, "やあ");
        assert_eq!(emotion.as_str(), "普通");
    }

    // ─── msg_handler の統合テスト（モック使用）───────────────

    fn valid_body() -> serde_json::Value {
        serde_json::json!({
            "character_name": "takochan",
            "version": "1.0.0",
            "user_name": "さのまる",
            "session_id": "ses-001",
            "message": "こんにちは"
        })
    }

    #[tokio::test]
    async fn msg_handler_returns_200_with_valid_request() {
        let mut lm = MockLmStudioClient::new();
        lm.expect_chat()
            .once()
            .returning(|_| Ok(lm_response("はじめまして！", "嬉しい")));
        let mut db = MockConversationRepository::new();
        db.expect_save_log().times(2).returning(|_| Ok(()));

        let (cfg, _tmp) = make_config();
        let server = make_server(lm, db, cfg);
        let res = server.post("/v1/msg").json(&valid_body()).await;

        res.assert_status(StatusCode::OK);
        let json = res.json::<serde_json::Value>();
        assert_eq!(json["message"], "はじめまして！");
        assert_eq!(json["emotion"], "嬉しい");
    }

    #[tokio::test]
    async fn msg_handler_returns_400_when_character_name_empty() {
        let (cfg, _tmp) = make_config();
        let server = make_server(
            MockLmStudioClient::new(),
            MockConversationRepository::new(),
            cfg,
        );
        let body = serde_json::json!({"character_name":"","version":"1.0.0","user_name":"さのまる","session_id":"ses-001","message":"こんにちは"});
        server
            .post("/v1/msg")
            .json(&body)
            .await
            .assert_status(StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn msg_handler_returns_400_when_user_name_empty() {
        let (cfg, _tmp) = make_config();
        let server = make_server(
            MockLmStudioClient::new(),
            MockConversationRepository::new(),
            cfg,
        );
        let body = serde_json::json!({"character_name":"takochan","version":"1.0.0","user_name":"","session_id":"ses-001","message":"こんにちは"});
        server
            .post("/v1/msg")
            .json(&body)
            .await
            .assert_status(StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn msg_handler_returns_400_when_message_empty() {
        let (cfg, _tmp) = make_config();
        let server = make_server(
            MockLmStudioClient::new(),
            MockConversationRepository::new(),
            cfg,
        );
        let body = serde_json::json!({"character_name":"takochan","version":"1.0.0","user_name":"さのまる","session_id":"ses-001","message":""});
        server
            .post("/v1/msg")
            .json(&body)
            .await
            .assert_status(StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn msg_handler_returns_502_when_lm_studio_fails() {
        let mut lm = MockLmStudioClient::new();
        lm.expect_chat()
            .once()
            .returning(|_| Err(crate::core::error::AppError::LmStudio("接続失敗".into())));

        let (cfg, _tmp) = make_config();
        let server = make_server(lm, MockConversationRepository::new(), cfg);
        server
            .post("/v1/msg")
            .json(&valid_body())
            .await
            .assert_status(StatusCode::BAD_GATEWAY);
    }

    #[tokio::test]
    async fn msg_handler_includes_response_id_in_reply() {
        let mut lm = MockLmStudioClient::new();
        lm.expect_chat()
            .once()
            .returning(|_| Ok(lm_response("やあ！", "楽しい")));
        let mut db = MockConversationRepository::new();
        db.expect_save_log().times(2).returning(|_| Ok(()));

        let (cfg, _tmp) = make_config();
        let server = make_server(lm, db, cfg);
        let res = server.post("/v1/msg").json(&valid_body()).await;

        let json = res.json::<serde_json::Value>();
        assert_eq!(json["response_id"], "resp-001");
    }
}
