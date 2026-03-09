/// summarize バイナリ
///
/// 既存の session テーブルから 25ターンごとにサマリを生成し、
/// session_summary テーブルへ保存する（リフレッシュ: DELETE → INSERT）。
///
/// 必須環境変数:
///   NEKOBOX_DB_PATH        SQLite ファイルのあるディレクトリ
///   NEKOBOX_LMSTUDIO_HOST  LM Studio ホスト名
///   NEKOBOX_LMSTUDIO_PORT  LM Studio ポート番号
///   NEKOBOX_MODEL_ID       使用するモデル ID
///   NEKOEXPERT_PATH        expert_summary_ja_v1.0.0.md があるディレクトリ
use anyhow::{Context, Result};
use async_trait::async_trait;
use nekobox_backend::{
    api::lm_studio::{ChatMessage, ChatRequest, HttpLmStudioClient, LmStudioClient},
    core::{
        error::AppError,
        summary::{
            generate_summaries, SqliteSummaryRepository, SummaryRepository, TextCompleter,
            TURNS_PER_CHUNK,
        },
    },
};
use tracing::info;
use tracing_subscriber::EnvFilter;

// ─────────────────────────────────────── TextCompleter 実装 ────────────────

/// `HttpLmStudioClient` を `TextCompleter` に適合させるアダプター
struct LmStudioCompleter {
    client: HttpLmStudioClient,
    model_id: String,
}

#[async_trait]
impl TextCompleter for LmStudioCompleter {
    async fn complete(&self, system: &str, user: &str) -> Result<String, AppError> {
        let request = ChatRequest {
            model: self.model_id.clone(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: Some(system.to_string()),
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: Some(user.to_string()),
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                },
            ],
            temperature: 0.3,
            tools: None,
            tool_choice: None,
        };

        let response = self.client.chat(request).await?;
        Ok(response
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message.content)
            .unwrap_or_default())
    }
}

// ─────────────────────────────────────── エントリポイント ──────────────────

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    // 環境変数を読み込む
    let db_path =
        std::env::var("NEKOBOX_DB_PATH").context("NEKOBOX_DB_PATH が設定されていません")?;
    let lm_host = std::env::var("NEKOBOX_LMSTUDIO_HOST")
        .context("NEKOBOX_LMSTUDIO_HOST が設定されていません")?;
    let lm_port = std::env::var("NEKOBOX_LMSTUDIO_PORT")
        .context("NEKOBOX_LMSTUDIO_PORT が設定されていません")?;
    // 未指定の場合は空文字列 → LM Studio がロード中のモデルを使用する
    let model_id = std::env::var("NEKOBOX_MODEL_ID").unwrap_or_default();
    let expert_path =
        std::env::var("NEKOEXPERT_PATH").context("NEKOEXPERT_PATH が設定されていません")?;

    // システムプロンプトを読み込む
    let prompt_file = std::path::Path::new(&expert_path).join("expert_summary_gen_ja_v1.0.0.md");
    let system_prompt = std::fs::read_to_string(&prompt_file).with_context(|| {
        format!(
            "システムプロンプトの読み込みに失敗: {}",
            prompt_file.display()
        )
    })?;

    // SQLite 接続 & マイグレーション
    let db_url = format!("sqlite:{db_path}/nekobox.sqlite3?mode=rwc");
    let pool = sqlx::SqlitePool::connect(&db_url)
        .await
        .context("SQLite 接続に失敗")?;
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("マイグレーションに失敗")?;

    let repo: Box<dyn SummaryRepository> = Box::new(SqliteSummaryRepository::new(pool));

    // LM Studio クライアント設定
    let lm_base_url = format!("http://{lm_host}:{lm_port}");
    let completer = LmStudioCompleter {
        client: HttpLmStudioClient::new(lm_base_url),
        model_id,
    };

    info!(
        "サマリ生成を開始します (turns_per_chunk={})",
        TURNS_PER_CHUNK
    );

    let stats = generate_summaries(repo.as_ref(), &completer, &system_prompt, TURNS_PER_CHUNK)
        .await
        .context("サマリ生成中にエラーが発生しました")?;

    info!(
        "サマリ生成完了: セッション {} 件処理, サマリ {} 件作成",
        stats.sessions_processed, stats.summaries_created
    );

    Ok(())
}
