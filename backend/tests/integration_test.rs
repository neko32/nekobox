//! 統合テスト
//! 実際の `SQLite` (in-memory) + 手動スタブ `LmStudioClient` で
//! エンドツーエンドのリクエスト → DB 保存 → レスポンスのフローを検証する。

use async_trait::async_trait;
use axum::{http::StatusCode, routing::post, Router};
use axum_test::TestServer;
use sqlx::SqlitePool;
use std::sync::Arc;

use nekobox_backend::{
    api::{
        lm_studio::{ChatChoice, ChatMessage, ChatRequest, ChatResponse, LmStudioClient},
        routes::msg::msg_handler,
    },
    core::{
        config::{AppConfig, CharacterConfig, ModelConfig},
        db::SqliteConversationRepository,
        error::AppError,
    },
    AppState,
};

// ─── LmStudioClient スタブ ────────────────────────────────────────────────

/// 成功レスポンスを返すスタブ
struct SuccessLmClient {
    message: String,
    emotion: String,
}

#[async_trait]
impl LmStudioClient for SuccessLmClient {
    async fn chat(&self, _req: ChatRequest) -> Result<ChatResponse, AppError> {
        Ok(ChatResponse {
            id: "stub-resp-001".to_string(),
            choices: vec![ChatChoice {
                message: ChatMessage {
                    role: "assistant".to_string(),
                    content: format!(
                        r#"{{"message":"{}","emotion":"{}"}}"#,
                        self.message, self.emotion
                    ),
                },
                finish_reason: Some("stop".to_string()),
            }],
            usage: None,
            model: Some("stub-model".to_string()),
        })
    }
}

/// エラーを返すスタブ
struct ErrorLmClient(String);

#[async_trait]
impl LmStudioClient for ErrorLmClient {
    async fn chat(&self, _req: ChatRequest) -> Result<ChatResponse, AppError> {
        Err(AppError::LmStudio(self.0.clone()))
    }
}

// ─── セットアップヘルパー ─────────────────────────────────────────────────

async fn setup_db() -> SqlitePool {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool
}

fn make_config(settings_dir: &std::path::Path) -> AppConfig {
    let prompt_md = "あなたはたこちゃんです。JSON形式で答えてください。";
    std::fs::write(settings_dir.join("takochan_1.0.0.md"), prompt_md).unwrap();
    AppConfig {
        current_session: "ses-it-001".to_string(),
        user_name: "さのまる".to_string(),
        background_image: Some("/bg.png".to_string()),
        character: CharacterConfig {
            name: "takochan".to_string(),
            version: "1.0.0".to_string(),
            model_path: None,
            settings_path: settings_dir.to_string_lossy().into_owned(),
        },
        model: ModelConfig { temperature: 0.6 },
    }
}

fn make_server(pool: SqlitePool, lm: Arc<dyn LmStudioClient>, config: AppConfig) -> TestServer {
    let state = Arc::new(AppState {
        db: Arc::new(SqliteConversationRepository::new(pool)),
        lm_client: lm,
        app_config: config,
        available_tools: vec![],
    });
    let app = Router::new()
        .route("/v1/msg", post(msg_handler))
        .with_state(state);
    TestServer::new(app)
}

fn valid_body() -> serde_json::Value {
    serde_json::json!({
        "character_name": "takochan",
        "version": "1.0.0",
        "user_name": "さのまる",
        "session_id": "ses-it-001",
        "message": "こんにちは！"
    })
}

async fn row_count(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM session")
        .fetch_one(pool)
        .await
        .unwrap()
}

// ─── テスト ──────────────────────────────────────────────────────────────

/// 正常系: 1リクエストで session テーブルに 2 行（ユーザー + キャラクター）保存される
#[tokio::test]
async fn full_flow_saves_two_logs_to_db() {
    let pool = setup_db().await;
    let pool_ref = pool.clone();
    let tmp = tempfile::tempdir().unwrap();
    let cfg = make_config(tmp.path());

    let lm = Arc::new(SuccessLmClient {
        message: "やあ！".to_string(),
        emotion: "嬉しい".to_string(),
    });

    let server = make_server(pool, lm, cfg);
    server
        .post("/v1/msg")
        .json(&valid_body())
        .await
        .assert_status(StatusCode::OK);

    assert_eq!(
        row_count(&pool_ref).await,
        2,
        "ユーザーとキャラクターの 2 件が session テーブルに保存されるはず"
    );
}

/// 正常系: レスポンス JSON の message / emotion / `session_id` が正しい
#[tokio::test]
async fn full_flow_response_json_is_correct() {
    let pool = setup_db().await;
    let tmp = tempfile::tempdir().unwrap();
    let cfg = make_config(tmp.path());

    let lm = Arc::new(SuccessLmClient {
        message: "こんにちは！たこちゃんです！".to_string(),
        emotion: "楽しい".to_string(),
    });

    let server = make_server(pool, lm, cfg);
    let res = server.post("/v1/msg").json(&valid_body()).await;

    res.assert_status(StatusCode::OK);

    let json = res.json::<serde_json::Value>();
    assert_eq!(json["message"], "こんにちは！たこちゃんです！");
    assert_eq!(json["emotion"], "楽しい");
    assert_eq!(json["session_id"], "ses-it-001");
    assert_eq!(json["character_name"], "takochan");
    assert_eq!(json["response_id"], "stub-resp-001");
}

/// バリデーションエラー: message が空の場合は 400 を返し DB には書き込まない
#[tokio::test]
async fn validation_error_does_not_write_to_db() {
    let pool = setup_db().await;
    let pool_ref = pool.clone();
    let tmp = tempfile::tempdir().unwrap();
    let cfg = make_config(tmp.path());

    // LM は呼ばれないのでエラースタブでよい（呼ばれたらパニック）
    let lm = Arc::new(ErrorLmClient("呼ばれるはずがない".to_string()));

    let server = make_server(pool, lm, cfg);
    let body = serde_json::json!({
        "character_name": "takochan",
        "version": "1.0.0",
        "user_name": "さのまる",
        "session_id": "ses-it-001",
        "message": ""
    });
    server
        .post("/v1/msg")
        .json(&body)
        .await
        .assert_status(StatusCode::BAD_REQUEST);

    assert_eq!(
        row_count(&pool_ref).await,
        0,
        "バリデーションエラー時は DB に書き込まない"
    );
}

/// LM Studio エラー: LM が失敗した場合は 502 を返し DB には書き込まない
#[tokio::test]
async fn lm_studio_error_does_not_write_to_db() {
    let pool = setup_db().await;
    let pool_ref = pool.clone();
    let tmp = tempfile::tempdir().unwrap();
    let cfg = make_config(tmp.path());

    let lm = Arc::new(ErrorLmClient("接続タイムアウト".to_string()));

    let server = make_server(pool, lm, cfg);
    server
        .post("/v1/msg")
        .json(&valid_body())
        .await
        .assert_status(StatusCode::BAD_GATEWAY);

    assert_eq!(
        row_count(&pool_ref).await,
        0,
        "LM エラー時は DB に書き込まない"
    );
}

/// 正常系: DB の送信者名がユーザーとキャラクターの順で記録される
#[tokio::test]
async fn full_flow_db_logs_have_correct_sender_names() {
    let pool = setup_db().await;
    let pool_ref = pool.clone();
    let tmp = tempfile::tempdir().unwrap();
    let cfg = make_config(tmp.path());

    let lm = Arc::new(SuccessLmClient {
        message: "よろしくまる！".to_string(),
        emotion: "普通".to_string(),
    });

    let server = make_server(pool, lm, cfg);
    server
        .post("/v1/msg")
        .json(&valid_body())
        .await
        .assert_status(StatusCode::OK);

    let senders: Vec<String> =
        sqlx::query_scalar("SELECT msg_sender_name FROM session ORDER BY id")
            .fetch_all(&pool_ref)
            .await
            .unwrap();

    assert_eq!(senders.len(), 2);
    assert_eq!(senders[0], "さのまる", "1 行目はユーザーの送信者名");
    assert_eq!(senders[1], "takochan", "2 行目はキャラクターの送信者名");
}
