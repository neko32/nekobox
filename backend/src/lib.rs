pub mod api;
pub mod core;

use std::sync::Arc;

use api::lm_studio::LmStudioClient;
use core::{config::AppConfig, db::ConversationRepository};

pub struct AppState {
    pub db: Arc<dyn ConversationRepository>,
    pub lm_client: Arc<dyn LmStudioClient>,
    pub app_config: AppConfig,
    /// uv tool list から取得した利用可能な MCP ツール名一覧
    pub available_tools: Vec<String>,
}
