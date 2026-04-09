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
        lm_studio::{ChatMessage, HttpLmStudioClient, LmStudioClient},
        routes,
    },
    core::{
        config::AppConfig,
        db::{ConversationRepository, SqliteConversationRepository},
        history::MessageHistory,
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

    // background_config.json ロード
    let background = app_config.load_background(&cfg_path)?;
    if let Some(ref bg) = background {
        info!("Background loaded: id={}, name={}", bg.id, bg.name);
    }

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

    // 短期記憶バッファを起動時に DB から復元
    let message_history = {
        let current_session = &app_config.current_session;
        let recent_logs = db.get_recent_turns(current_session, 25).await?;
        let turns = build_history_turns(recent_logs);
        let mut history = MessageHistory::new(25, current_session.clone());
        for turn in turns {
            history.push_turn(turn);
        }
        info!(
            "Message history loaded: {} turn(s) for session={}",
            history.len(),
            current_session
        );
        Arc::new(tokio::sync::Mutex::new(history))
    };

    let state = Arc::new(AppState {
        db,
        lm_client,
        app_config,
        background,
        available_tools,
        mcp_provider,
        message_history,
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

/// `SessionLog` のリストをターン単位の `Vec<Vec<ChatMessage>>` に変換する。
///
/// `turn_number` でグルーピングし、各グループ内を timestamp ASC 順で
/// `ChatMessage` に変換して返す。グループは `turn_number` ASC 順。
fn build_history_turns(
    logs: Vec<nekobox_backend::core::models::SessionLog>,
) -> Vec<Vec<ChatMessage>> {
    // turn_number → Vec<ChatMessage> のマップ（挿入順を保つため BTreeMap を使用）
    let mut map: std::collections::BTreeMap<i64, Vec<ChatMessage>> =
        std::collections::BTreeMap::new();

    for log in logs {
        // tool ロールは tool_call_id が不明なため content のみ保持する
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

    if server_names.is_empty() {
        info!("MCP servers: (none installed)");
    } else {
        info!("MCP servers detected: {}", server_names.join(", "));
    }

    // 各サーバーからツール定義を取得
    let mut all_tools = Vec::new();
    for name in &server_names {
        match provider.list_tools(name).await {
            Ok(tools) => {
                let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
                info!(
                    "MCP server '{}': {} tool(s) [{}]",
                    name,
                    tools.len(),
                    tool_names.join(", ")
                );
                all_tools.extend(tools);
            }
            Err(e) => {
                tracing::warn!("Failed to load tools from MCP server '{name}': {e}");
            }
        }
    }
    all_tools
}
