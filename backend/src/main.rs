use anyhow::Result;
use axum::{
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::EnvFilter;

use nekobox_backend::{
    api::{
        lm_studio::{HttpLmStudioClient, LmStudioClient},
        routes,
    },
    core::{
        config::AppConfig,
        db::{ConversationRepository, SqliteConversationRepository},
        mcp::{parse_uv_tool_list, McpToolProvider, UvMcpToolProvider},
    },
    AppState,
};

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    // 必須環境変数チェック
    let db_path = std::env::var("NEKOBOX_DB_PATH").expect("NEKOBOX_DB_PATH is required");
    let lm_host =
        std::env::var("NEKOBOX_LMSTUDIO_HOST").expect("NEKOBOX_LMSTUDIO_HOST is required");
    let lm_port =
        std::env::var("NEKOBOX_LMSTUDIO_PORT").expect("NEKOBOX_LMSTUDIO_PORT is required");
    let cfg_path = std::env::var("NEKOBOX_CFG_PATH").expect("NEKOBOX_CFG_PATH is required");

    // app.config ロード
    let app_config = AppConfig::load(&cfg_path)?;

    // SQLite 接続 & マイグレーション
    let db_url = format!("sqlite:{db_path}/nekobox.sqlite3?mode=rwc");
    let pool = sqlx::SqlitePool::connect(&db_url).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;

    // 依存注入
    let db: Arc<dyn ConversationRepository> = Arc::new(SqliteConversationRepository::new(pool));
    let lm_base_url = format!("http://{lm_host}:{lm_port}");
    let lm_client: Arc<dyn LmStudioClient> = Arc::new(HttpLmStudioClient::new(lm_base_url));

    // MCP プロバイダー初期化
    let mcp_provider: Arc<dyn McpToolProvider> = Arc::new(UvMcpToolProvider);

    // uv tool list でサーバー名を取得し、各サーバーのツール定義を収集
    let available_tools = collect_mcp_tools(&mcp_provider).await;
    info!("MCP tools loaded: {} tools total", available_tools.len());

    let state = Arc::new(AppState {
        db,
        lm_client,
        app_config,
        available_tools,
        mcp_provider,
    });

    let app = Router::new()
        .route("/v1/msg", post(routes::msg::msg_handler))
        .route(
            "/v1/sessions/{session_id}",
            get(routes::sessions::sessions_handler),
        )
        .with_state(state);

    // ローカルコンパニオンアプリのデフォルトは 127.0.0.1（ループバックのみ）。
    // Docker コンテナ内では NEKOBOX_BIND_HOST=0.0.0.0 に設定すること。
    let bind_host = std::env::var("NEKOBOX_BIND_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let addr = format!("{bind_host}:8080");
    info!("nekobox backend listening on {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// `uv tool list` で MCP サーバー名を取得し、各サーバーのツール定義を収集する
async fn collect_mcp_tools(
    provider: &Arc<dyn McpToolProvider>,
) -> Vec<nekobox_backend::core::mcp::McpToolDefinition> {
    // uv tool list を実行してサーバー名一覧を取得
    let server_names = match tokio::process::Command::new("uv")
        .args(["tool", "list"])
        .output()
        .await
    {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            parse_uv_tool_list(&stdout)
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!("uv tool list failed: {stderr}");
            return vec![];
        }
        Err(e) => {
            tracing::warn!("Failed to run uv tool list: {e}");
            return vec![];
        }
    };

    // 各サーバーからツール定義を取得
    let mut all_tools = Vec::new();
    for name in &server_names {
        match provider.list_tools(name).await {
            Ok(tools) => {
                info!("Loaded {} tool(s) from MCP server '{name}'", tools.len());
                all_tools.extend(tools);
            }
            Err(e) => {
                tracing::warn!("Failed to load tools from MCP server '{name}': {e}");
            }
        }
    }
    all_tools
}
