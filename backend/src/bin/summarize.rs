/// summarize バイナリ
///
/// 既存の session テーブルから 25ターンごとにサマリを生成し、
/// session_summary テーブルへ保存する（リフレッシュ: DELETE → INSERT）。
///
/// 必須環境変数:
///   NEKOBOX_DB_PATH        SQLite ファイルのあるディレクトリ
///   NEKOBOX_LMSTUDIO_HOST  LM Studio ホスト名
///   NEKOBOX_LMSTUDIO_PORT  LM Studio ポート番号
///   NEKOBOX_CFG_PATH       app.config があるディレクトリ
///   NEKOEXPERT_PATH        expert_summary_gen_ja_v1.0.0.md があるディレクトリ
///
/// オプション環境変数:
///   NEKOBOX_MODEL_ID       使用するモデル ID（未指定時は LM Studio ロード中のモデルを使用）
use anyhow::{Context, Result};
use nekobox_backend::{
    api::lm_studio::{HttpLmStudioClient, LmStudioTextCompleter},
    core::{
        config::AppConfig,
        summary::{
            generate_summaries, SqliteSummaryRepository, SummaryRepository, TURNS_PER_CHUNK,
        },
    },
};
use tracing::info;
use tracing_subscriber::EnvFilter;

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
    let cfg_path =
        std::env::var("NEKOBOX_CFG_PATH").context("NEKOBOX_CFG_PATH が設定されていません")?;
    let expert_path =
        std::env::var("NEKOEXPERT_PATH").context("NEKOEXPERT_PATH が設定されていません")?;

    // app.config を読み込む
    let app_config = AppConfig::load(&cfg_path).context("app.config の読み込みに失敗")?;
    let summary_gen_temperature = app_config.model.summary_gen.temperature;
    info!("summary_gen temperature: {summary_gen_temperature}");

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
    let completer = LmStudioTextCompleter {
        client: HttpLmStudioClient::new(lm_base_url),
        model_id,
        temperature: summary_gen_temperature,
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
