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

// ───────────────────────────────────── TextCompleter アダプター ────────────

/// `LmStudioClient` を `core::summary::TextCompleter` に適合させるアダプター
///
/// system プロンプトと user メッセージを受け取り、LM Studio へ送信して
/// アシスタントの返答文字列を返す。
pub struct LmStudioTextCompleter<C: LmStudioClient> {
    pub client: C,
    pub model_id: String,
}

#[async_trait]
impl<C: LmStudioClient> crate::core::summary::TextCompleter for LmStudioTextCompleter<C> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::summary::TextCompleter;

    #[tokio::test]
    async fn completer_returns_assistant_content() {
        let mut mock_client = MockLmStudioClient::new();
        mock_client.expect_chat().returning(|_| {
            Ok(ChatResponse {
                id: "resp-1".to_string(),
                choices: vec![ChatChoice {
                    message: ChatMessage {
                        role: "assistant".to_string(),
                        content: Some("サマリ結果".to_string()),
                        tool_calls: None,
                        tool_call_id: None,
                        name: None,
                    },
                    finish_reason: Some("stop".to_string()),
                }],
                usage: None,
                model: None,
            })
        });

        let completer = LmStudioTextCompleter {
            client: mock_client,
            model_id: String::new(),
        };

        let result = completer.complete("system", "user").await.unwrap();
        assert_eq!(result, "サマリ結果");
    }

    #[tokio::test]
    async fn completer_returns_empty_string_when_no_choices() {
        let mut mock_client = MockLmStudioClient::new();
        mock_client.expect_chat().returning(|_| {
            Ok(ChatResponse {
                id: "resp-2".to_string(),
                choices: vec![],
                usage: None,
                model: None,
            })
        });

        let completer = LmStudioTextCompleter {
            client: mock_client,
            model_id: String::new(),
        };

        let result = completer.complete("system", "user").await.unwrap();
        assert_eq!(result, "");
    }

    #[tokio::test]
    async fn completer_sends_correct_model_id() {
        let mut mock_client = MockLmStudioClient::new();
        mock_client
            .expect_chat()
            .withf(|req| req.model == "test-model-123")
            .returning(|_| {
                Ok(ChatResponse {
                    id: "resp-3".to_string(),
                    choices: vec![],
                    usage: None,
                    model: None,
                })
            });

        let completer = LmStudioTextCompleter {
            client: mock_client,
            model_id: "test-model-123".to_string(),
        };

        completer.complete("sys", "usr").await.unwrap();
    }

    #[tokio::test]
    async fn completer_sends_system_and_user_messages() {
        let mut mock_client = MockLmStudioClient::new();
        mock_client
            .expect_chat()
            .withf(|req| {
                req.messages.len() == 2
                    && req.messages[0].role == "system"
                    && req.messages[0].content.as_deref() == Some("システムプロンプト")
                    && req.messages[1].role == "user"
                    && req.messages[1].content.as_deref() == Some("会話テキスト")
            })
            .returning(|_| {
                Ok(ChatResponse {
                    id: "resp-4".to_string(),
                    choices: vec![],
                    usage: None,
                    model: None,
                })
            });

        let completer = LmStudioTextCompleter {
            client: mock_client,
            model_id: String::new(),
        };

        completer
            .complete("システムプロンプト", "会話テキスト")
            .await
            .unwrap();
    }
}
