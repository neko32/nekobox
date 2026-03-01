pub mod api;
pub mod core;

use std::sync::Arc;

use api::lm_studio::LmStudioClient;
use core::{config::AppConfig, db::ConversationRepository};

pub struct AppState {
    pub db: Arc<dyn ConversationRepository>,
    pub lm_client: Arc<dyn LmStudioClient>,
    pub app_config: AppConfig,
}
