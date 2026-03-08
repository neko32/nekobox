pub mod api;
pub mod core;

use std::sync::Arc;

use api::lm_studio::LmStudioClient;
use core::{
    config::AppConfig,
    db::ConversationRepository,
    mcp::{McpToolDefinition, McpToolProvider},
};

pub struct AppState {
    pub db: Arc<dyn ConversationRepository>,
    pub lm_client: Arc<dyn LmStudioClient>,
    pub app_config: AppConfig,
    /// MCP サーバーから取得した利用可能なツール定義一覧
    pub available_tools: Vec<McpToolDefinition>,
    /// MCP ツールの呼び出しプロバイダー
    pub mcp_provider: Arc<dyn McpToolProvider>,
}
