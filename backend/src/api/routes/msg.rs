use axum::{extract::State, Json};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{
    api::lm_studio::{ChatMessage, ChatRequest, FunctionSpec, ToolSpec},
    core::{
        error::AppError,
        mcp::McpToolDefinition,
        models::{Emotion, Role, SessionLog},
    },
    AppState,
};

/// ツール呼び出しの最大ループ回数（無限ループ防止）
const MAX_TOOL_ITERATIONS: usize = 5;

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

#[allow(clippy::too_many_lines)]
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

    // LM Studio へ送る初期メッセージを構築
    let mut messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: Some(system_prompt),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        },
        ChatMessage {
            role: "user".to_string(),
            content: Some(req.message.clone()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        },
    ];

    // 利用可能なツールを OpenAI function calling 形式に変換
    let tools = build_tool_specs(&state.available_tools);
    let tool_choice = tools.as_ref().map(|_| "auto".to_string());

    // ── 最初の LM Studio 呼び出し（エラー時は DB に書き込まない）──
    let first_request = ChatRequest {
        model: state.app_config.character.name.clone(),
        messages: messages.clone(),
        temperature: state.app_config.model.temperature,
        tools: tools.clone(),
        tool_choice: tool_choice.clone(),
    };
    let mut lm_response = state.lm_client.chat(first_request).await?;

    // 最初の LM 呼び出し成功後にターン番号を算出し、ユーザーメッセージを保存
    let settings_name = format!("{}_{}", req.character_name, req.version);
    let bg = state
        .app_config
        .background_image
        .clone()
        .unwrap_or_default();
    let turn_number = state.db.get_current_turn(&req.session_id).await? + 1;

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
            role: Role::User,
            emotion: None,
            turn_number,
        })
        .await?;

    // ── ツール呼び出しループ ────────────────────────────────────────────────
    // ツールループ中は turn_number を変えない（同一ターン内の処理）
    let (final_resp_id, final_model, final_usage, final_content) = 'tool_loop: {
        for _iteration in 0..MAX_TOOL_ITERATIONS {
            let resp_id = lm_response.id.clone();
            let resp_model = lm_response.model.clone();
            let resp_usage = lm_response.usage;

            let choice = lm_response.choices.into_iter().next().ok_or_else(|| {
                AppError::LmStudio("LM Studioのレスポンスにchoicesがありません".into())
            })?;

            // tool_calls がない、または空の場合は最終レスポンスとして break
            let has_tool_calls = choice
                .message
                .tool_calls
                .as_ref()
                .is_some_and(|v| !v.is_empty());

            if !has_tool_calls {
                break 'tool_loop (resp_id, resp_model, resp_usage, choice.message.content);
            }

            // ─ ツール呼び出し処理 ─
            let tool_calls = choice.message.tool_calls.clone().unwrap_or_default();

            // アシスタントのメッセージ（tool_calls 付き）を会話履歴に追加
            messages.push(choice.message);

            for tc in &tool_calls {
                let args: serde_json::Value =
                    serde_json::from_str(&tc.function.arguments).unwrap_or_default();

                let server_cmd = find_server_command(&state.available_tools, &tc.function.name);

                // MCP サーバーを呼び出す（エラーはテキストとして処理を継続）
                let tool_result = state
                    .mcp_provider
                    .call_tool(&server_cmd, &tc.function.name, args)
                    .await
                    .unwrap_or_else(|e| format!("ツールエラー: {e}"));

                // ツール結果を DB に保存（同一ターン番号）
                state
                    .db
                    .save_log(&SessionLog {
                        session_id: req.session_id.clone(),
                        session_alias: req.session_alias.clone(),
                        background_image: bg.clone(),
                        msg_sender_name: tc.function.name.clone(),
                        user_name: req.user_name.clone(),
                        settings_name: settings_name.clone(),
                        msg: tool_result.clone(),
                        image_url: None,
                        response_id: None,
                        model_instance_id: None,
                        input_tokens: None,
                        total_output_tokens: None,
                        timestamp: Utc::now(),
                        role: Role::Tool,
                        emotion: None,
                        turn_number,
                    })
                    .await?;

                // ツール結果を会話履歴に追加
                messages.push(ChatMessage {
                    role: "tool".to_string(),
                    content: Some(tool_result),
                    tool_calls: None,
                    tool_call_id: Some(tc.id.clone()),
                    name: Some(tc.function.name.clone()),
                });
            }

            // 更新した会話履歴で LM Studio を再度呼び出す
            let next_request = ChatRequest {
                model: state.app_config.character.name.clone(),
                messages: messages.clone(),
                temperature: state.app_config.model.temperature,
                tools: tools.clone(),
                tool_choice: tool_choice.clone(),
            };
            lm_response = state.lm_client.chat(next_request).await?;
        }

        return Err(AppError::LmStudio(
            "ツールループが最大イテレーション数を超えました".into(),
        ));
    };

    let new_response_id = Some(final_resp_id);
    let model_instance_id = final_model;
    let (input_tokens, output_tokens) = final_usage
        .as_ref()
        .map_or((None, None), |u| (u.prompt_tokens, u.completion_tokens));

    // LM Studio レスポンスから message と emotion を抽出
    let raw_content = final_content.unwrap_or_default();
    let (character_message, emotion) = parse_lm_response(&raw_content);

    // キャラクターのレスポンスをDBに保存（同じターン番号）
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
            role: Role::Assistant,
            emotion: Some(emotion.as_str().to_string()),
            turn_number,
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

// ───────────────────────────────────── ヘルパー ────────────────────────────

/// 利用可能なツール定義を LM Studio 用の ToolSpec リストに変換する
fn build_tool_specs(tools: &[McpToolDefinition]) -> Option<Vec<ToolSpec>> {
    if tools.is_empty() {
        return None;
    }
    Some(
        tools
            .iter()
            .map(|t| ToolSpec {
                spec_type: "function".to_string(),
                function: FunctionSpec {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: t.input_schema.clone(),
                },
            })
            .collect(),
    )
}

/// ツール名からサーバーコマンドを検索する（見つからなければツール名をそのまま返す）
fn find_server_command(tools: &[McpToolDefinition], tool_name: &str) -> String {
    tools
        .iter()
        .find(|t| t.name == tool_name)
        .map_or_else(|| tool_name.to_string(), |t| t.server_command.clone())
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
            mcp::MockMcpToolProvider,
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
                    content: Some(format!(r#"{{"message":"{msg}","emotion":"{emotion}"}}"#)),
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
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
            available_tools: vec![],
            mcp_provider: Arc::new(MockMcpToolProvider::new()),
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
        db.expect_get_current_turn().once().returning(|_| Ok(0));
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
        // LM Studioのエラーはget_current_turnより前に発生するためDBは呼ばれない
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
        db.expect_get_current_turn().once().returning(|_| Ok(0));
        db.expect_save_log().times(2).returning(|_| Ok(()));

        let (cfg, _tmp) = make_config();
        let server = make_server(lm, db, cfg);
        let res = server.post("/v1/msg").json(&valid_body()).await;

        let json = res.json::<serde_json::Value>();
        assert_eq!(json["response_id"], "resp-001");
    }

    /// T5相当: 新規セッション（turn=0）で1ターン目、既存セッション（turn=1）で2ターン目
    #[tokio::test]
    async fn msg_handler_assigns_correct_turn_number() {
        // ターン1: get_current_turn=0 → save_log は turn_number=1 で呼ばれるはず
        let mut lm = MockLmStudioClient::new();
        lm.expect_chat()
            .once()
            .returning(|_| Ok(lm_response("1回目の返答", "嬉しい")));
        let mut db = MockConversationRepository::new();
        db.expect_get_current_turn().once().returning(|_| Ok(0));
        db.expect_save_log()
            .times(2)
            .withf(|log| log.turn_number == 1)
            .returning(|_| Ok(()));

        let (cfg, _tmp) = make_config();
        let server = make_server(lm, db, cfg);
        server
            .post("/v1/msg")
            .json(&valid_body())
            .await
            .assert_status(StatusCode::OK);

        // ターン2: get_current_turn=1 → save_log は turn_number=2 で呼ばれるはず
        let mut lm2 = MockLmStudioClient::new();
        lm2.expect_chat()
            .once()
            .returning(|_| Ok(lm_response("2回目の返答", "楽しい")));
        let mut db2 = MockConversationRepository::new();
        db2.expect_get_current_turn().once().returning(|_| Ok(1));
        db2.expect_save_log()
            .times(2)
            .withf(|log| log.turn_number == 2)
            .returning(|_| Ok(()));

        let (cfg2, _tmp2) = make_config();
        let server2 = make_server(lm2, db2, cfg2);
        server2
            .post("/v1/msg")
            .json(&valid_body())
            .await
            .assert_status(StatusCode::OK);
    }

    /// ツール呼び出しループ: LMがtool_callsを返し、ツール実行後に最終レスポンスを返すケース
    #[tokio::test]
    async fn msg_handler_executes_tool_call_and_returns_final_response() {
        use crate::api::lm_studio::{ToolCall, ToolCallFunction};
        use crate::core::mcp::McpToolDefinition;

        // 1回目: tool_calls を返す
        let tool_call_response = ChatResponse {
            id: "resp-tool".to_string(),
            choices: vec![ChatChoice {
                message: ChatMessage {
                    role: "assistant".to_string(),
                    content: None,
                    tool_calls: Some(vec![ToolCall {
                        id: "call-001".to_string(),
                        call_type: "function".to_string(),
                        function: ToolCallFunction {
                            name: "weather_get".to_string(),
                            arguments: r#"{"city":"Tokyo"}"#.to_string(),
                        },
                    }]),
                    tool_call_id: None,
                    name: None,
                },
                finish_reason: Some("tool_calls".to_string()),
            }],
            usage: None,
            model: Some("takochan".to_string()),
        };

        // 2回目: 最終レスポンス
        let final_response = lm_response("東京は晴れです！", "嬉しい");

        let mut lm = MockLmStudioClient::new();
        let mut call_count = 0usize;
        lm.expect_chat().times(2).returning(move |_| {
            call_count += 1;
            if call_count == 1 {
                Ok(ChatResponse {
                    id: "resp-tool".to_string(),
                    choices: vec![ChatChoice {
                        message: ChatMessage {
                            role: "assistant".to_string(),
                            content: None,
                            tool_calls: Some(vec![ToolCall {
                                id: "call-001".to_string(),
                                call_type: "function".to_string(),
                                function: ToolCallFunction {
                                    name: "weather_get".to_string(),
                                    arguments: r#"{"city":"Tokyo"}"#.to_string(),
                                },
                            }]),
                            tool_call_id: None,
                            name: None,
                        },
                        finish_reason: Some("tool_calls".to_string()),
                    }],
                    usage: None,
                    model: Some("takochan".to_string()),
                })
            } else {
                Ok(lm_response("東京は晴れです！", "嬉しい"))
            }
        });

        let mut db = MockConversationRepository::new();
        db.expect_get_current_turn().once().returning(|_| Ok(0));
        // user(1) + tool(1) + assistant(1) = 3回 save_log が呼ばれる
        db.expect_save_log()
            .times(3)
            .withf(|log| log.turn_number == 1)
            .returning(|_| Ok(()));

        let mut mcp = MockMcpToolProvider::new();
        mcp.expect_call_tool()
            .once()
            .returning(|_, _, _| Ok("東京の天気: 晴れ, 25℃".to_string()));

        let (cfg, _tmp) = make_config();
        let tool_def = McpToolDefinition {
            name: "weather_get".to_string(),
            description: Some("天気を取得する".to_string()),
            input_schema: serde_json::json!({}),
            server_command: "weather-mcp".to_string(),
        };
        let state = Arc::new(AppState {
            db: Arc::new(db),
            lm_client: Arc::new(lm),
            app_config: cfg,
            available_tools: vec![tool_def],
            mcp_provider: Arc::new(mcp),
        });

        let _ = tool_call_response; // suppress unused warning
        let _ = final_response;

        let app = Router::new()
            .route("/v1/msg", post(msg_handler))
            .with_state(state);
        let server = TestServer::new(app);

        let res = server.post("/v1/msg").json(&valid_body()).await;
        res.assert_status(StatusCode::OK);
        let json = res.json::<serde_json::Value>();
        assert_eq!(json["message"], "東京は晴れです！");
        assert_eq!(json["emotion"], "嬉しい");
    }
}
