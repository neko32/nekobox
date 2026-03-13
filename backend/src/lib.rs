pub mod api;
pub mod core;

use std::sync::Arc;

use api::lm_studio::{ChatMessage, LmStudioClient};
use core::{
    config::{AppConfig, BackgroundEntry},
    db::ConversationRepository,
    history::MessageHistory,
    mcp::{McpToolDefinition, McpToolProvider},
};
use tokio::sync::Mutex;

pub struct AppState {
    pub db: Arc<dyn ConversationRepository>,
    pub lm_client: Arc<dyn LmStudioClient>,
    pub app_config: AppConfig,
    /// ロード済みの背景エントリ（background_id に一致するもの）
    pub background: Option<BackgroundEntry>,
    /// MCP サーバーから取得した利用可能なツール定義一覧
    pub available_tools: Vec<McpToolDefinition>,
    /// MCP ツールの呼び出しプロバイダー
    pub mcp_provider: Arc<dyn McpToolProvider>,
    /// OpenAI-compat Chat Completion 用の短期記憶バッファ（最大25ターン）
    pub message_history: Arc<Mutex<MessageHistory<ChatMessage>>>,
}
