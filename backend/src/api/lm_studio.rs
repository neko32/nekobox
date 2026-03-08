use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::core::error::AppError;

// ───────────────────────────────────── Tool structs ────────────────────────

/// LM Studio へ渡すツール定義（OpenAI function calling 形式）
#[derive(Debug, Serialize, Clone)]
pub struct ToolSpec {
    #[serde(rename = "type")]
    pub spec_type: String,
    pub function: FunctionSpec,
}

#[derive(Debug, Serialize, Clone)]
pub struct FunctionSpec {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub parameters: serde_json::Value,
}

/// LM Studio からのレスポンス中のツール呼び出し情報
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: ToolCallFunction,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ToolCallFunction {
    pub name: String,
    /// JSON 文字列として返されるツール引数
    pub arguments: String,
}

// ───────────────────────────────────── Request ─────────────────────────────

#[derive(Debug, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolSpec>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// アシスタントがツールを呼び出す際に設定される
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    /// ツール結果メッセージの場合に対応する tool_call の ID を設定する
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// ツール結果メッセージのツール名（任意）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

// ───────────────────────────────────── Response ────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ChatResponse {
    pub id: String,
    pub choices: Vec<ChatChoice>,
    pub usage: Option<Usage>,
    pub model: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChatChoice {
    pub message: ChatMessage,
    pub finish_reason: Option<String>,
}

// LM Studio API のフィールド名をそのまま使うため allow
#[allow(clippy::struct_field_names)]
#[derive(Debug, Deserialize)]
pub struct Usage {
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
}

// ───────────────────────────────────── Trait ───────────────────────────────

/// LM Studio APIクライアントのトレイト（テスト時はモックに差し替え可）
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait LmStudioClient: Send + Sync {
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, AppError>;
}

// ───────────────────────────────────── 実装 ────────────────────────────────

pub struct HttpLmStudioClient {
    client: reqwest::Client,
    base_url: String,
}

impl HttpLmStudioClient {
    #[must_use]
    pub fn new(base_url: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url,
        }
    }
}

#[async_trait]
impl LmStudioClient for HttpLmStudioClient {
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, AppError> {
        let url = format!("{}/v1/chat/completions", self.base_url);
        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await?
            .error_for_status()
            .map_err(|e| AppError::LmStudio(e.to_string()))?
            .json::<ChatResponse>()
            .await?;
        Ok(response)
    }
}
