use async_trait::async_trait;
use tokio::process::Command;

use crate::core::error::AppError;

// ───────────────────────────────────── Trait ───────────────────────────────

/// MCPツールリストの取得トレイト（テスト時はモックに差し替え可）
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait McpToolProvider: Send + Sync {
    async fn list_tools(&self) -> Result<Vec<String>, AppError>;
}

// ───────────────────────────────────── 実装 ────────────────────────────────

/// `uv tool list` の出力から MCPツール名を収集する実装
pub struct UvMcpToolProvider;

#[async_trait]
impl McpToolProvider for UvMcpToolProvider {
    async fn list_tools(&self) -> Result<Vec<String>, AppError> {
        let output = Command::new("uv")
            .args(["tool", "list"])
            .output()
            .await
            .map_err(|e| AppError::Mcp(format!("uv tool list failed: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::Mcp(format!(
                "uv tool list exited with error: {stderr}"
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(parse_uv_tool_list(&stdout))
    }
}

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
}
