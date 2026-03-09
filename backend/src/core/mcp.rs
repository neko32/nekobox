use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::time;

use crate::core::error::AppError;

/// MCP セッションのタイムアウト秒数
const MCP_TIMEOUT_SECS: u64 = 5;

// ───────────────────────────────────── 型 ──────────────────────────────────

/// MCP サーバーから取得したツール定義
#[derive(Debug, Clone)]
pub struct McpToolDefinition {
    /// ツール名（例: `weather_get`）
    pub name: String,
    /// ツールの説明
    pub description: Option<String>,
    /// JSON Schema 形式の入力スキーマ
    pub input_schema: Value,
    /// このツールを提供するサーバーコマンド（uv tool run に渡すパッケージ名）
    pub server_command: String,
}

// ───────────────────────────────────── Trait ───────────────────────────────

/// MCPツール操作のトレイト（テスト時はモックに差し替え可）
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait McpToolProvider: Send + Sync {
    /// MCP サーバーを起動してツール定義一覧を取得する
    async fn list_tools(&self, server_command: &str) -> Result<Vec<McpToolDefinition>, AppError>;

    /// MCP サーバーを起動してツールを呼び出し、テキスト結果を返す
    async fn call_tool(
        &self,
        server_command: &str,
        tool_name: &str,
        arguments: Value,
    ) -> Result<String, AppError>;
}

// ───────────────────────────────────── 実装 ────────────────────────────────

/// `uv tool run <server_command>` で MCP サーバーを起動する実装
pub struct UvMcpToolProvider;

#[async_trait]
impl McpToolProvider for UvMcpToolProvider {
    async fn list_tools(&self, server_command: &str) -> Result<Vec<McpToolDefinition>, AppError> {
        #[derive(Deserialize)]
        struct ToolsListResult {
            tools: Vec<McpToolRaw>,
        }
        #[derive(Deserialize)]
        struct McpToolRaw {
            name: String,
            description: Option<String>,
            #[serde(rename = "inputSchema", default)]
            input_schema: Value,
        }

        let result = mcp_communicate(server_command, "tools/list", None, 2).await?;

        let parsed: ToolsListResult = serde_json::from_value(result)
            .map_err(|e| AppError::Mcp(format!("tools/list parse error: {e}")))?;

        Ok(parsed
            .tools
            .into_iter()
            .map(|t| McpToolDefinition {
                name: t.name,
                description: t.description,
                input_schema: t.input_schema,
                server_command: server_command.to_string(),
            })
            .collect())
    }

    async fn call_tool(
        &self,
        server_command: &str,
        tool_name: &str,
        arguments: Value,
    ) -> Result<String, AppError> {
        #[derive(Deserialize)]
        struct CallResult {
            content: Vec<McpContent>,
        }
        #[derive(Deserialize)]
        struct McpContent {
            #[serde(rename = "type")]
            content_type: String,
            #[serde(default)]
            text: String,
        }

        let result = mcp_communicate(
            server_command,
            "tools/call",
            Some(json!({ "name": tool_name, "arguments": arguments })),
            2,
        )
        .await?;

        let parsed: CallResult = serde_json::from_value(result)
            .map_err(|e| AppError::Mcp(format!("tools/call parse error: {e}")))?;

        let text = parsed
            .content
            .into_iter()
            .filter(|c| c.content_type == "text")
            .map(|c| c.text)
            .collect::<Vec<_>>()
            .join("\n");

        Ok(text)
    }
}

// ───────────────────────────────────── MCP JSON-RPC ────────────────────────

/// MCP サーバーを spawn して JSON-RPC セッションを実行する
async fn mcp_communicate(
    server_command: &str,
    method: &str,
    params: Option<Value>,
    request_id: u64,
) -> Result<Value, AppError> {
    let mut child = Command::new("uv")
        .args(["tool", "run", server_command])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| AppError::Mcp(format!("cannot spawn {server_command}: {e}")))?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| AppError::Mcp("stdin unavailable".into()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AppError::Mcp("stdout unavailable".into()))?;

    let outcome = time::timeout(
        Duration::from_secs(MCP_TIMEOUT_SECS),
        do_mcp_exchange(&mut stdin, stdout, method, params, request_id),
    )
    .await
    .map_err(|_| {
        AppError::Mcp(format!(
            "MCP timeout after {MCP_TIMEOUT_SECS}s for {server_command}"
        ))
    });

    let _ = child.kill().await;

    match outcome {
        Err(e) => Err(e),
        Ok(inner) => inner,
    }
}

/// MCP プロトコルの実際の I/O を担う非同期関数
/// stdin/stdout をジェネリック型にすることでテスト時にメモリ上のパイプを注入できる
async fn do_mcp_exchange<W, R>(
    stdin: &mut W,
    stdout: R,
    method: &str,
    params: Option<Value>,
    request_id: u64,
) -> Result<Value, AppError>
where
    W: AsyncWrite + Unpin,
    R: AsyncRead + Unpin,
{
    let mut reader = BufReader::new(stdout);

    // 1. initialize リクエスト送信
    write_jsonrpc(
        stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "nekobox", "version": "0.1.0"}
            }
        }),
    )
    .await?;

    // 2. initialize レスポンス受信（id=1）
    read_jsonrpc_response(&mut reader, 1).await?;

    // 3. notifications/initialized 送信（通知なのでIDなし）
    write_jsonrpc(
        stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }),
    )
    .await?;

    // 4. 実際のリクエスト送信
    let request = if let Some(p) = params {
        json!({ "jsonrpc": "2.0", "id": request_id, "method": method, "params": p })
    } else {
        json!({ "jsonrpc": "2.0", "id": request_id, "method": method })
    };
    write_jsonrpc(stdin, request).await?;

    // 5. 実際のレスポンス受信
    read_jsonrpc_response(&mut reader, request_id).await
}

/// JSON-RPC メッセージを stdin に改行区切りで書き込む
async fn write_jsonrpc<W: AsyncWrite + Unpin>(stdin: &mut W, msg: Value) -> Result<(), AppError> {
    let s =
        serde_json::to_string(&msg).map_err(|e| AppError::Mcp(format!("serialize error: {e}")))?;
    stdin
        .write_all(s.as_bytes())
        .await
        .map_err(|e| AppError::Mcp(format!("stdin write error: {e}")))?;
    stdin
        .write_all(b"\n")
        .await
        .map_err(|e| AppError::Mcp(format!("stdin newline write error: {e}")))?;
    Ok(())
}

/// 期待する id のレスポンスを stdout から読み取る（通知行はスキップ）
async fn read_jsonrpc_response<R: AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
    expected_id: u64,
) -> Result<Value, AppError> {
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader
            .read_line(&mut line)
            .await
            .map_err(|e| AppError::Mcp(format!("stdout read error: {e}")))?;
        if n == 0 {
            return Err(AppError::Mcp(
                "MCP server closed stdout unexpectedly".into(),
            ));
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let resp: Value = serde_json::from_str(trimmed)
            .map_err(|e| AppError::Mcp(format!("JSON parse error ({e}): {trimmed}")))?;

        // RPC エラーチェック
        if let Some(err) = resp.get("error") {
            return Err(AppError::Mcp(format!("MCP RPC error: {err}")));
        }

        // 期待する id のレスポンスか確認
        if resp.get("id").and_then(serde_json::Value::as_u64) == Some(expected_id) {
            return resp
                .get("result")
                .cloned()
                .ok_or_else(|| AppError::Mcp("MCP response missing 'result' field".into()));
        }
        // 通知など id が異なるメッセージはスキップ
    }
}

// ───────────────────────────────────── ヘルパー ────────────────────────────

/// `uv tool list` のstdoutをパースして `- ` で始まる行のツール名リストを返す
#[must_use]
pub fn parse_uv_tool_list(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| line.strip_prefix("- "))
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .collect()
}

// ───────────────────────────────────── テスト ──────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_uv_tool_list ────────────────────────────────────

    #[test]
    fn parse_uv_tool_list_extracts_tool_names() {
        let output = "takochan 1.0.0\n- takochan\n- takochan-mcp\nnekobox 0.1.0\n- nekobox\n";
        let tools = parse_uv_tool_list(output);
        assert_eq!(tools, vec!["takochan", "takochan-mcp", "nekobox"]);
    }

    #[test]
    fn parse_uv_tool_list_empty_output_returns_empty() {
        let tools = parse_uv_tool_list("");
        assert!(tools.is_empty());
    }

    #[test]
    fn parse_uv_tool_list_no_dash_lines_returns_empty() {
        let output = "takochan 1.0.0\nnekobox 0.1.0\n";
        let tools = parse_uv_tool_list(output);
        assert!(tools.is_empty());
    }

    #[test]
    fn parse_uv_tool_list_ignores_empty_after_prefix() {
        let output = "- \n- valid-tool\n";
        let tools = parse_uv_tool_list(output);
        assert_eq!(tools, vec!["valid-tool"]);
    }

    #[test]
    fn parse_uv_tool_list_trims_whitespace() {
        let output = "-   spaced-tool  \n";
        let tools = parse_uv_tool_list(output);
        assert_eq!(tools, vec!["spaced-tool"]);
    }

    // ── フェイク MCP サーバーヘルパー ─────────────────────────

    /// フェイクの MCP サーバーとして `do_mcp_exchange` に渡す stdin/stdout を構築し、
    /// サーバー側の応答を別タスクで書き込む。
    ///
    /// `server_responses` には、サーバーが送信する改行区切り JSON-RPC メッセージの
    /// リストを渡す（initialize レスポンス → 実際のメソッドのレスポンス の順）。
    async fn fake_mcp_exchange(
        method: &str,
        params: Option<Value>,
        request_id: u64,
        server_responses: Vec<&'static str>,
    ) -> Result<Value, AppError> {
        // 全サーバーレスポンスを事前に連結してインメモリバッファを作成する
        let server_data: Vec<u8> = server_responses
            .iter()
            .flat_map(|r| r.as_bytes().iter().copied())
            .collect();

        // stdin: クライアントの書き込みはすべて破棄（sink）
        let mut fake_stdin = tokio::io::sink();
        // stdout: 事前に用意したサーバーデータを返す（Cursor が EOF まで読める）
        let fake_stdout = std::io::Cursor::new(server_data);

        do_mcp_exchange(&mut fake_stdin, fake_stdout, method, params, request_id).await
    }

    // ── do_mcp_exchange の正常系テスト ───────────────────────

    #[tokio::test]
    async fn exchange_list_tools_returns_result() {
        let init_resp = "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{}}}\n";
        let tools_resp = "{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"tools\":[{\"name\":\"test_tool\",\"inputSchema\":{}}]}}\n";

        let result = fake_mcp_exchange("tools/list", None, 2, vec![init_resp, tools_resp])
            .await
            .unwrap();

        assert_eq!(result["tools"][0]["name"], "test_tool");
    }

    #[tokio::test]
    async fn exchange_call_tool_returns_result() {
        let init_resp = "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"capabilities\":{}}}\n";
        let call_resp = "{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"Tokyo: sunny\"}]}}\n";

        let result = fake_mcp_exchange(
            "tools/call",
            Some(json!({"name":"weather","arguments":{}})),
            2,
            vec![init_resp, call_resp],
        )
        .await
        .unwrap();

        assert_eq!(result["content"][0]["text"], "Tokyo: sunny");
    }

    #[tokio::test]
    async fn exchange_skips_notifications_and_finds_correct_id() {
        // サーバーが id=9 の通知を挟んでも id=2 のレスポンスを正しく取得できる
        let init_resp = "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n";
        let notif = "{\"jsonrpc\":\"2.0\",\"method\":\"some/notification\",\"params\":{}}\n";
        let actual_resp = "{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"value\":\"found\"}}\n";

        let result = fake_mcp_exchange("tools/list", None, 2, vec![init_resp, notif, actual_resp])
            .await
            .unwrap();

        assert_eq!(result["value"], "found");
    }

    #[tokio::test]
    async fn exchange_returns_error_on_rpc_error_response() {
        let init_resp = "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n";
        let err_resp = "{\"jsonrpc\":\"2.0\",\"id\":2,\"error\":{\"code\":-32601,\"message\":\"Method not found\"}}\n";

        let err = fake_mcp_exchange("tools/list", None, 2, vec![init_resp, err_resp])
            .await
            .unwrap_err();

        assert!(matches!(err, AppError::Mcp(_)));
        assert!(err.to_string().contains("MCP RPC error"));
    }

    #[tokio::test]
    async fn exchange_returns_error_on_unexpected_eof() {
        // サーバーが initialize レスポンスだけ返して切断する
        let init_resp = "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n";

        let err = fake_mcp_exchange("tools/list", None, 2, vec![init_resp])
            .await
            .unwrap_err();

        assert!(matches!(err, AppError::Mcp(_)));
    }

    #[tokio::test]
    async fn exchange_returns_error_on_invalid_json() {
        let init_resp = "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n";
        let bad_json = "not valid json at all\n";

        let err = fake_mcp_exchange("tools/list", None, 2, vec![init_resp, bad_json])
            .await
            .unwrap_err();

        assert!(matches!(err, AppError::Mcp(_)));
        assert!(err.to_string().contains("JSON parse error"));
    }

    #[tokio::test]
    async fn exchange_returns_error_when_result_missing() {
        let init_resp = "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n";
        // result フィールドがない
        let no_result = "{\"jsonrpc\":\"2.0\",\"id\":2}\n";

        let err = fake_mcp_exchange("tools/list", None, 2, vec![init_resp, no_result])
            .await
            .unwrap_err();

        assert!(matches!(err, AppError::Mcp(_)));
        assert!(err.to_string().contains("missing 'result'"));
    }

    // ── write_jsonrpc ─────────────────────────────────────────

    #[tokio::test]
    async fn write_jsonrpc_appends_newline() {
        let mut buf = Vec::new();
        write_jsonrpc(&mut buf, json!({"key": "value"}))
            .await
            .unwrap();
        assert!(buf.ends_with(b"\n"));
        let s = std::str::from_utf8(&buf).unwrap().trim();
        let parsed: Value = serde_json::from_str(s).unwrap();
        assert_eq!(parsed["key"], "value");
    }

    // ── UvMcpToolProvider parse helpers ───────────────────────

    #[tokio::test]
    async fn list_tools_parses_valid_response() {
        // list_tools は内部で mcp_communicate を使うため、
        // 結果値のパーシングロジックだけを直接テストする

        let raw = json!({
            "tools": [
                {"name": "tool_a", "description": "desc a", "inputSchema": {"type": "object"}},
                {"name": "tool_b", "inputSchema": {}}
            ]
        });

        // パーシングのみを検証（subprocess を起動しない）
        #[derive(Deserialize)]
        struct ToolsListResult {
            tools: Vec<McpToolRaw>,
        }
        #[derive(Deserialize)]
        struct McpToolRaw {
            name: String,
            description: Option<String>,
            #[serde(rename = "inputSchema", default)]
            #[allow(dead_code)]
            input_schema: Value,
        }

        let parsed: ToolsListResult = serde_json::from_value(raw).unwrap();
        assert_eq!(parsed.tools.len(), 2);
        assert_eq!(parsed.tools[0].name, "tool_a");
        assert_eq!(parsed.tools[0].description.as_deref(), Some("desc a"));
        assert!(parsed.tools[1].description.is_none());
    }

    #[tokio::test]
    async fn call_tool_parses_text_content() {
        let raw = json!({
            "content": [
                {"type": "text", "text": "hello"},
                {"type": "image", "text": "should be ignored"},
                {"type": "text", "text": "world"}
            ]
        });

        #[derive(Deserialize)]
        struct CallResult {
            content: Vec<McpContent>,
        }
        #[derive(Deserialize)]
        struct McpContent {
            #[serde(rename = "type")]
            content_type: String,
            #[serde(default)]
            text: String,
        }

        let parsed: CallResult = serde_json::from_value(raw).unwrap();
        let text = parsed
            .content
            .into_iter()
            .filter(|c| c.content_type == "text")
            .map(|c| c.text)
            .collect::<Vec<_>>()
            .join("\n");

        assert_eq!(text, "hello\nworld");
    }
}
