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

/// 短期記憶の最大ターン数
const MAX_HISTORY_TURNS: usize = 25;
/// DB クエリ用の最大ターン数（i64）
const MAX_HISTORY_TURNS_I64: i64 = MAX_HISTORY_TURNS as i64;

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
    let mut system_prompt = state.app_config.load_system_prompt()?;

    // 背景情報をシステムプロンプトに追記
    if let Some(ref bg) = state.background {
        let tags = bg.location_type.join(", ");
        system_prompt.push_str(&format!(
            "\n\n# 追加情報: 背景とその設定\n名前: {}\n説明: {}\n場所のタグ: {{{}}}",
            bg.name, bg.description, tags
        ));
    }

    // ── 短期記憶バッファの取得・セッション切り替え検出 ──────────────────────
    let mut history = state.message_history.lock().await;
    if history.session_id() != req.session_id {
        // /new コマンド等によるセッション切り替え → DB から新セッションの履歴を復元
        tracing::info!(
            "Session changed: {} → {}. Reloading history.",
            history.session_id(),
            req.session_id
        );
        let recent_logs = state
            .db
            .get_recent_turns(&req.session_id, MAX_HISTORY_TURNS_I64)
            .await?;
        let turns = build_history_turns_from_logs(recent_logs);
        history.reset(req.session_id.clone(), turns);
    }

    // LM Studio へ送るメッセージを構築: system + 過去の履歴 + 今回の user メッセージ
    let current_user_msg = ChatMessage {
        role: "user".to_string(),
        content: Some(req.message.clone()),
        tool_calls: None,
        tool_call_id: None,
        name: None,
    };
    let mut messages = Vec::with_capacity(2 + history.to_messages().len() + 1);
    messages.push(ChatMessage {
        role: "system".to_string(),
        content: Some(system_prompt),
        tool_calls: None,
        tool_call_id: None,
        name: None,
    });
    messages.extend(history.to_messages());
    messages.push(current_user_msg.clone());

    // ロックを解放してI/Oを進める（DB保存・LM呼び出し中はロック不要）
    drop(history);

    // 利用可能なツールを OpenAI function calling 形式に変換
    let tools = build_tool_specs(&state.available_tools);
    let tool_choice = tools.as_ref().map(|_| "auto".to_string());

    // ── 最初の LM Studio 呼び出し（エラー時は DB に書き込まない）──
    let regular_chat_temperature = state.app_config.model.regular_chat.temperature;
    tracing::info!("msg_handler: regular_chat temperature={regular_chat_temperature}");
    let first_request = ChatRequest {
        model: state.app_config.character.name.clone(),
        messages: messages.clone(),
        temperature: regular_chat_temperature,
        tools: tools.clone(),
        tool_choice: tool_choice.clone(),
    };
    let mut lm_response = state.lm_client.chat(first_request).await?;

    // 最初の LM 呼び出し成功後にターン番号を算出し、ユーザーメッセージを保存
    let settings_name = format!("{}_{}", req.character_name, req.version);
    let bg = state
        .background
        .as_ref()
        .map(|b| b.image.clone())
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
                temperature: regular_chat_temperature,
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

    // ── 短期記憶バッファに今回のターンを追加 ────────────────────────────────
    // turn_msgs: [user, (tool...,) assistant] の順で 1ターン分をまとめて push
    {
        let mut history = state.message_history.lock().await;
        // ツールループ中のメッセージを収集する（tool_calls 付きアシスタント＋ツール結果）
        // messages は [system, ...history, user, (tool_calls_assistant, tool_result)*] の構造
        // system と history 部分を除いた残りが今ターン分
        let history_len = history.to_messages().len();
        // システムメッセージ(1) + 過去履歴 を除いた今ターン分
        let this_turn_start = 1 + history_len;
        let mut turn_msgs: Vec<ChatMessage> = messages[this_turn_start..].to_vec();
        // 最終 assistant レスポンスを追加
        turn_msgs.push(ChatMessage {
            role: "assistant".to_string(),
            content: Some(character_message.clone()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        });
        history.push_turn(turn_msgs);
    }

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

/// `SessionLog` のリストをターン単位の `Vec<Vec<ChatMessage>>` に変換する。
///
/// `turn_number` でグルーピングし、各グループ内を挿入順で `ChatMessage` に変換する。
fn build_history_turns_from_logs(
    logs: Vec<crate::core::models::SessionLog>,
) -> Vec<Vec<ChatMessage>> {
    let mut map: std::collections::BTreeMap<i64, Vec<ChatMessage>> =
        std::collections::BTreeMap::new();
    for log in logs {
        let msg = ChatMessage {
            role: log.role.as_str().to_string(),
            content: Some(log.msg),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        };
        map.entry(log.turn_number).or_default().push(msg);
    }
    map.into_values().collect()
}

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
            config::{AppConfig, CharacterConfig, ChatModelConfig, ModelConfig},
            db::MockConversationRepository,
            history::MessageHistory,
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
            background_id: Some("forest_0001".to_string()),
            character: CharacterConfig {
                name: "takochan".to_string(),
                version: "1.0.0".to_string(),
                model_path: None,
                settings_path: tmp.path().to_string_lossy().into_owned(),
            },
            model: ModelConfig {
                regular_chat: ChatModelConfig { temperature: 0.6 },
                summary_gen: ChatModelConfig { temperature: 0.1 },
            },
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

    fn make_history(
        session_id: &str,
    ) -> Arc<tokio::sync::Mutex<MessageHistory<crate::api::lm_studio::ChatMessage>>> {
        Arc::new(tokio::sync::Mutex::new(MessageHistory::new(25, session_id)))
    }

    fn make_server(
        lm: MockLmStudioClient,
        db: MockConversationRepository,
        config: AppConfig,
    ) -> TestServer {
        let session_id = config.current_session.clone();
        let state = Arc::new(AppState {
            db: Arc::new(db),
            lm_client: Arc::new(lm),
            app_config: config,
            background: None,
            available_tools: vec![],
            mcp_provider: Arc::new(MockMcpToolProvider::new()),
            message_history: make_history(&session_id),
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
            background: None,
            available_tools: vec![tool_def],
            mcp_provider: Arc::new(mcp),
            message_history: make_history("ses-001"),
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

    /// 履歴があるとき、LM Studio へのリクエストに過去の会話が含まれることを確認
    #[tokio::test]
    async fn msg_handler_includes_history_in_request() {
        let mut lm = MockLmStudioClient::new();
        lm.expect_chat()
            .once()
            .withf(|req| {
                // system(1) + history(2: user+assistant) + current_user(1) = 4
                req.messages.len() == 4
                    && req.messages[1].role == "user"
                    && req.messages[1].content.as_deref() == Some("前回のユーザーメッセージ")
                    && req.messages[2].role == "assistant"
                    && req.messages[3].role == "user"
                    && req.messages[3].content.as_deref() == Some("こんにちは")
            })
            .returning(|_| Ok(lm_response("返答です！", "嬉しい")));

        let mut db = MockConversationRepository::new();
        db.expect_get_current_turn().once().returning(|_| Ok(1));
        db.expect_save_log().times(2).returning(|_| Ok(()));

        let (cfg, _tmp) = make_config();
        let session_id = cfg.current_session.clone();

        // 履歴に1ターン分（user + assistant）を事前投入
        let history = {
            let mut h = MessageHistory::new(25, &session_id);
            h.push_turn(vec![
                crate::api::lm_studio::ChatMessage {
                    role: "user".to_string(),
                    content: Some("前回のユーザーメッセージ".to_string()),
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                },
                crate::api::lm_studio::ChatMessage {
                    role: "assistant".to_string(),
                    content: Some("前回のアシスタント返答".to_string()),
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                },
            ]);
            Arc::new(tokio::sync::Mutex::new(h))
        };

        let state = Arc::new(AppState {
            db: Arc::new(db),
            lm_client: Arc::new(lm),
            app_config: cfg,
            background: None,
            available_tools: vec![],
            mcp_provider: Arc::new(MockMcpToolProvider::new()),
            message_history: history,
        });
        let app = Router::new()
            .route("/v1/msg", post(msg_handler))
            .with_state(state);
        let server = TestServer::new(app);

        server
            .post("/v1/msg")
            .json(&valid_body())
            .await
            .assert_status(StatusCode::OK);
    }

    /// レスポンス成功後、履歴に今回のターンが追加されることを確認
    #[tokio::test]
    async fn msg_handler_updates_history_after_response() {
        let mut lm = MockLmStudioClient::new();
        lm.expect_chat()
            .once()
            .returning(|_| Ok(lm_response("返答です！", "嬉しい")));

        let mut db = MockConversationRepository::new();
        db.expect_get_current_turn().once().returning(|_| Ok(0));
        db.expect_save_log().times(2).returning(|_| Ok(()));

        let (cfg, _tmp) = make_config();
        let session_id = cfg.current_session.clone();
        let history = Arc::new(tokio::sync::Mutex::new(MessageHistory::new(
            25,
            &session_id,
        )));
        let history_clone = Arc::clone(&history);

        let state = Arc::new(AppState {
            db: Arc::new(db),
            lm_client: Arc::new(lm),
            app_config: cfg,
            background: None,
            available_tools: vec![],
            mcp_provider: Arc::new(MockMcpToolProvider::new()),
            message_history: history,
        });
        let app = Router::new()
            .route("/v1/msg", post(msg_handler))
            .with_state(state);
        let server = TestServer::new(app);

        server
            .post("/v1/msg")
            .json(&valid_body())
            .await
            .assert_status(StatusCode::OK);

        // リクエスト後、履歴に1ターン分追加されているはず
        let h = history_clone.lock().await;
        assert_eq!(h.len(), 1, "履歴に1ターン追加されているべき");
        let msgs = h.to_messages();
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[0].content.as_deref(), Some("こんにちは"));
        assert_eq!(msgs[1].role, "assistant");
        assert_eq!(msgs[1].content.as_deref(), Some("返答です！"));
    }

    /// セッションIDが変わったとき、履歴がフラッシュされ新セッションで再ロードされることを確認
    #[tokio::test]
    async fn msg_handler_flushes_history_on_session_change() {
        let mut lm = MockLmStudioClient::new();
        lm.expect_chat()
            .once()
            .withf(|req| {
                // 新セッション → 履歴なし → system(1) + user(1) = 2
                req.messages.len() == 2
                    && req.messages[0].role == "system"
                    && req.messages[1].role == "user"
            })
            .returning(|_| Ok(lm_response("新セッションの返答！", "普通")));

        let mut db = MockConversationRepository::new();
        // セッション切り替え時に get_recent_turns が呼ばれる（新セッションは空）
        db.expect_get_recent_turns()
            .once()
            .returning(|_, _| Ok(vec![]));
        db.expect_get_current_turn().once().returning(|_| Ok(0));
        db.expect_save_log().times(2).returning(|_| Ok(()));

        let (cfg, _tmp) = make_config();

        // 履歴は old-session に紐づいている（今回のリクエストは ses-001）
        let history = Arc::new(tokio::sync::Mutex::new(MessageHistory::new(
            25,
            "old-session-id", // ← リクエストの ses-001 と異なる
        )));
        let history_clone = Arc::clone(&history);

        // 古い履歴を1ターン投入
        {
            let mut h = history.lock().await;
            h.push_turn(vec![crate::api::lm_studio::ChatMessage {
                role: "user".to_string(),
                content: Some("古いセッションのメッセージ".to_string()),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            }]);
        }

        let state = Arc::new(AppState {
            db: Arc::new(db),
            lm_client: Arc::new(lm),
            app_config: cfg,
            background: None,
            available_tools: vec![],
            mcp_provider: Arc::new(MockMcpToolProvider::new()),
            message_history: history,
        });
        let app = Router::new()
            .route("/v1/msg", post(msg_handler))
            .with_state(state);
        let server = TestServer::new(app);

        // valid_body() は session_id = "ses-001"
        server
            .post("/v1/msg")
            .json(&valid_body())
            .await
            .assert_status(StatusCode::OK);

        // 履歴は ses-001 に切り替わり、今回のターンが1件入っている
        let h = history_clone.lock().await;
        assert_eq!(h.session_id(), "ses-001");
        assert_eq!(h.len(), 1);
    }

    /// 背景が設定されているとき、system プロンプトに背景情報が追記されることを確認
    #[tokio::test]
    async fn msg_handler_appends_background_info_to_system_prompt() {
        use crate::core::config::BackgroundEntry;

        let mut lm = MockLmStudioClient::new();
        lm.expect_chat().once().returning(|req| {
            // system メッセージに背景情報が含まれていることを検証
            let system_content = req
                .messages
                .iter()
                .find(|m| m.role == "system")
                .and_then(|m| m.content.as_deref())
                .unwrap_or("");
            assert!(
                system_content.contains("# 追加情報: 背景とその設定"),
                "system には背景情報が含まれるべき: {system_content}"
            );
            assert!(system_content.contains("森の神秘的な池"));
            assert!(system_content.contains("透明な池"));
            assert!(system_content.contains("屋外"));
            Ok(lm_response("背景を見ているよ！", "嬉しい"))
        });

        let mut db = MockConversationRepository::new();
        db.expect_get_current_turn().once().returning(|_| Ok(0));
        db.expect_save_log().times(2).returning(|_| Ok(()));

        let (cfg, _tmp) = make_config();
        let bg = BackgroundEntry {
            id: "forest_0001".to_string(),
            name: "森の神秘的な池".to_string(),
            image: "/images/forest.png".to_string(),
            description: "透明な池ときれいな木々".to_string(),
            location_type: vec!["屋外".to_string(), "自然".to_string()],
        };

        let state = Arc::new(AppState {
            db: Arc::new(db),
            lm_client: Arc::new(lm),
            app_config: cfg,
            background: Some(bg),
            available_tools: vec![],
            mcp_provider: Arc::new(MockMcpToolProvider::new()),
            message_history: make_history("ses-001"),
        });
        let app = Router::new()
            .route("/v1/msg", post(msg_handler))
            .with_state(state);
        let server = TestServer::new(app);

        server
            .post("/v1/msg")
            .json(&valid_body())
            .await
            .assert_status(StatusCode::OK);
    }
}
