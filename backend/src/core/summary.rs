use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{Row, SqlitePool};

use crate::core::error::AppError;

pub const TURNS_PER_CHUNK: i64 = 25;
pub const SUMMARY_UPDATED_BY: &str = "summary_gen";

// ─────────────────────────────────────── モデル ────────────────────────────

/// session_summary テーブル1行分
#[derive(Debug, Clone)]
pub struct SessionSummary {
    pub session_id: String,
    pub summary_id: i64,
    pub start_message_id: i64,
    pub end_message_id: i64,
    pub summary: String,
    pub updated_by: String,
    pub last_updated: DateTime<Utc>,
}

/// サマリ生成に使うセッションメッセージ行（id付き）
#[derive(Debug, Clone)]
pub struct SessionMessageRow {
    pub id: i64,
    pub turn_number: i64,
    pub role: String,
    pub msg: String,
}

// ─────────────────────────────────────── Repository trait ──────────────────

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait SummaryRepository: Send + Sync {
    /// 全セッション ID を重複なしで返す
    async fn get_all_session_ids(&self) -> Result<Vec<String>, AppError>;
    /// 指定セッションのメッセージを id 昇順で返す
    async fn get_messages_for_summary(
        &self,
        session_id: &str,
    ) -> Result<Vec<SessionMessageRow>, AppError>;
    /// 指定セッションのサマリをすべて削除する
    async fn delete_summaries_for_session(&self, session_id: &str) -> Result<(), AppError>;
    /// サマリを1件挿入する
    async fn insert_summary(&self, summary: &SessionSummary) -> Result<(), AppError>;
}

// ─────────────────────────────────────── TextCompleter trait ───────────────

/// LM Studio 等テキスト生成クライアントの抽象（循環依存を避けるため api に依存しない）
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait TextCompleter: Send + Sync {
    /// system プロンプトと user メッセージを送り、アシスタントの返答文字列を得る
    async fn complete(&self, system: &str, user: &str) -> Result<String, AppError>;
}

// ─────────────────────────────────────── SQLite 実装 ───────────────────────

pub struct SqliteSummaryRepository {
    pool: SqlitePool,
}

impl SqliteSummaryRepository {
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SummaryRepository for SqliteSummaryRepository {
    async fn get_all_session_ids(&self) -> Result<Vec<String>, AppError> {
        let rows = sqlx::query("SELECT DISTINCT session_id FROM session ORDER BY session_id ASC")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| r.get::<String, _>("session_id"))
            .collect())
    }

    async fn get_messages_for_summary(
        &self,
        session_id: &str,
    ) -> Result<Vec<SessionMessageRow>, AppError> {
        let rows = sqlx::query(
            r"SELECT id, turn_number, role, msg
              FROM session
              WHERE session_id = ?
              ORDER BY id ASC",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| SessionMessageRow {
                id: r.get("id"),
                turn_number: r.get("turn_number"),
                role: r.get("role"),
                msg: r.get("msg"),
            })
            .collect())
    }

    async fn delete_summaries_for_session(&self, session_id: &str) -> Result<(), AppError> {
        sqlx::query("DELETE FROM session_summary WHERE session_id = ?")
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn insert_summary(&self, summary: &SessionSummary) -> Result<(), AppError> {
        let last_updated = summary.last_updated.to_rfc3339();
        sqlx::query(
            r"INSERT INTO session_summary
              (session_id, summary_id, start_message_id, end_message_id,
               summary, updated_by, last_updated)
              VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&summary.session_id)
        .bind(summary.summary_id)
        .bind(summary.start_message_id)
        .bind(summary.end_message_id)
        .bind(&summary.summary)
        .bind(&summary.updated_by)
        .bind(&last_updated)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

// ─────────────────────────────────────── ビジネスロジック ──────────────────

/// メッセージリストを `turns_per_chunk` ターンごとのチャンクに分割する
///
/// turn_number が 1 始まりであることを前提とし、
/// `chunk_index = (turn_number - 1) / turns_per_chunk` で分類する。
/// turn_number が 0 のメッセージはチャンク 0 に含める。
#[must_use]
pub fn chunk_messages(messages: &[SessionMessageRow], turns_per_chunk: i64) -> Vec<Vec<usize>> {
    if messages.is_empty() || turns_per_chunk <= 0 {
        return vec![];
    }

    let mut chunks: Vec<Vec<usize>> = vec![];
    let mut current: Vec<usize> = vec![];
    let mut current_idx: i64 = -1;

    for (i, msg) in messages.iter().enumerate() {
        let chunk_idx = if msg.turn_number <= 0 {
            0
        } else {
            (msg.turn_number - 1) / turns_per_chunk
        };

        if chunk_idx != current_idx {
            if !current.is_empty() {
                chunks.push(current);
                current = vec![];
            }
            current_idx = chunk_idx;
        }
        current.push(i);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

/// チャンク内のメッセージから会話テキストを組み立てる
#[must_use]
pub fn build_conversation_text(messages: &[&SessionMessageRow]) -> String {
    messages
        .iter()
        .map(|m| {
            let label = match m.role.as_str() {
                "user" => "ユーザー",
                "assistant" => "アシスタント",
                other => other,
            };
            format!("[{label}]: {}", m.msg)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// サマリ統計
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SummaryStats {
    pub sessions_processed: usize,
    pub summaries_created: usize,
}

/// 全セッションのサマリをリフレッシュ生成するメインロジック
///
/// 各セッションの既存サマリを DELETE してから再生成する。
pub async fn generate_summaries(
    repo: &dyn SummaryRepository,
    completer: &dyn TextCompleter,
    system_prompt: &str,
    turns_per_chunk: i64,
) -> Result<SummaryStats, AppError> {
    let session_ids = repo.get_all_session_ids().await?;
    let mut stats = SummaryStats::default();

    for session_id in &session_ids {
        tracing::info!("session {session_id}: サマリ生成開始");

        repo.delete_summaries_for_session(session_id).await?;

        let messages = repo.get_messages_for_summary(session_id).await?;
        if messages.is_empty() {
            tracing::warn!("session {session_id}: メッセージなし、スキップ");
            continue;
        }

        let chunks = chunk_messages(&messages, turns_per_chunk);

        for (chunk_idx, indices) in chunks.iter().enumerate() {
            let summary_id = (chunk_idx + 1) as i64;
            let chunk_msgs: Vec<&SessionMessageRow> =
                indices.iter().map(|&i| &messages[i]).collect();

            let start_message_id = chunk_msgs.first().map(|m| m.id).unwrap_or(0);
            let end_message_id = chunk_msgs.last().map(|m| m.id).unwrap_or(0);
            let start_turn = chunk_msgs.first().map(|m| m.turn_number).unwrap_or(0);
            let end_turn = chunk_msgs.last().map(|m| m.turn_number).unwrap_or(0);

            let conversation_text = build_conversation_text(&chunk_msgs);

            tracing::info!(
                "session {session_id}: summary_id={summary_id} turns {start_turn}-{end_turn} \
                 (msg id {start_message_id}-{end_message_id}) を生成中"
            );

            let summary_text = completer
                .complete(system_prompt, &conversation_text)
                .await?;

            let summary = SessionSummary {
                session_id: session_id.clone(),
                summary_id,
                start_message_id,
                end_message_id,
                summary: summary_text,
                updated_by: SUMMARY_UPDATED_BY.to_string(),
                last_updated: Utc::now(),
            };

            repo.insert_summary(&summary).await?;
            stats.summaries_created += 1;
        }

        stats.sessions_processed += 1;
        tracing::info!("session {session_id}: サマリ生成完了");
    }

    Ok(stats)
}

// ─────────────────────────────────────── テスト ────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use mockall::predicate::*;

    // ── chunk_messages ─────────────────────────────────────────────────────

    fn make_msg(id: i64, turn: i64) -> SessionMessageRow {
        SessionMessageRow {
            id,
            turn_number: turn,
            role: "user".to_string(),
            msg: format!("メッセージ{id}"),
        }
    }

    #[test]
    fn chunk_messages_empty() {
        assert!(chunk_messages(&[], 25).is_empty());
    }

    #[test]
    fn chunk_messages_single_chunk() {
        let msgs: Vec<_> = (1i64..=25).map(|t| make_msg(t, t)).collect();
        let chunks = chunk_messages(&msgs, 25);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].len(), 25);
    }

    #[test]
    fn chunk_messages_two_chunks() {
        // 26ターン → chunk 0: turn 1-25, chunk 1: turn 26
        let msgs: Vec<_> = (1i64..=26).map(|t| make_msg(t, t)).collect();
        let chunks = chunk_messages(&msgs, 25);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 25);
        assert_eq!(chunks[1].len(), 1);
    }

    #[test]
    fn chunk_messages_exact_two_chunks() {
        let msgs: Vec<_> = (1i64..=50).map(|t| make_msg(t, t)).collect();
        let chunks = chunk_messages(&msgs, 25);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 25);
        assert_eq!(chunks[1].len(), 25);
    }

    #[test]
    fn chunk_messages_multiple_rows_per_turn() {
        // user + assistant で同じ turn_number を持つ場合
        let msgs = vec![
            SessionMessageRow {
                id: 1,
                turn_number: 1,
                role: "user".into(),
                msg: "u1".into(),
            },
            SessionMessageRow {
                id: 2,
                turn_number: 1,
                role: "assistant".into(),
                msg: "a1".into(),
            },
            SessionMessageRow {
                id: 3,
                turn_number: 2,
                role: "user".into(),
                msg: "u2".into(),
            },
            SessionMessageRow {
                id: 4,
                turn_number: 2,
                role: "assistant".into(),
                msg: "a2".into(),
            },
        ];
        let chunks = chunk_messages(&msgs, 25);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].len(), 4);
    }

    #[test]
    fn chunk_messages_zero_turn_goes_to_first_chunk() {
        let msgs = vec![make_msg(1, 0), make_msg(2, 1), make_msg(3, 25)];
        let chunks = chunk_messages(&msgs, 25);
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn chunk_messages_invalid_turns_per_chunk_returns_empty() {
        let msgs = vec![make_msg(1, 1)];
        assert!(chunk_messages(&msgs, 0).is_empty());
    }

    // ── build_conversation_text ────────────────────────────────────────────

    #[test]
    fn build_conversation_text_user_and_assistant() {
        let msgs = [
            SessionMessageRow {
                id: 1,
                turn_number: 1,
                role: "user".into(),
                msg: "こんにちは".into(),
            },
            SessionMessageRow {
                id: 2,
                turn_number: 1,
                role: "assistant".into(),
                msg: "やあ！".into(),
            },
        ];
        let refs: Vec<&SessionMessageRow> = msgs.iter().collect();
        let text = build_conversation_text(&refs);
        assert!(text.contains("[ユーザー]: こんにちは"));
        assert!(text.contains("[アシスタント]: やあ！"));
    }

    #[test]
    fn build_conversation_text_unknown_role_uses_raw() {
        let msgs = [SessionMessageRow {
            id: 1,
            turn_number: 1,
            role: "tool".into(),
            msg: "result".into(),
        }];
        let refs: Vec<&SessionMessageRow> = msgs.iter().collect();
        let text = build_conversation_text(&refs);
        assert!(text.contains("[tool]: result"));
    }

    // ── generate_summaries (モック使用) ────────────────────────────────────

    #[tokio::test]
    async fn generate_summaries_no_sessions() {
        let mut mock_repo = MockSummaryRepository::new();
        mock_repo
            .expect_get_all_session_ids()
            .returning(|| Ok(vec![]));

        let mock_completer = MockTextCompleter::new();

        let stats = generate_summaries(&mock_repo, &mock_completer, "system", 25)
            .await
            .unwrap();

        assert_eq!(stats.sessions_processed, 0);
        assert_eq!(stats.summaries_created, 0);
    }

    #[tokio::test]
    async fn generate_summaries_empty_session_is_skipped() {
        let mut mock_repo = MockSummaryRepository::new();
        mock_repo
            .expect_get_all_session_ids()
            .returning(|| Ok(vec!["ses-001".to_string()]));
        mock_repo
            .expect_delete_summaries_for_session()
            .with(eq("ses-001"))
            .returning(|_| Ok(()));
        mock_repo
            .expect_get_messages_for_summary()
            .with(eq("ses-001"))
            .returning(|_| Ok(vec![]));

        let mock_completer = MockTextCompleter::new();

        let stats = generate_summaries(&mock_repo, &mock_completer, "system", 25)
            .await
            .unwrap();

        assert_eq!(stats.sessions_processed, 0);
        assert_eq!(stats.summaries_created, 0);
    }

    #[tokio::test]
    async fn generate_summaries_single_chunk() {
        let mut mock_repo = MockSummaryRepository::new();
        mock_repo
            .expect_get_all_session_ids()
            .returning(|| Ok(vec!["ses-001".to_string()]));
        mock_repo
            .expect_delete_summaries_for_session()
            .with(eq("ses-001"))
            .returning(|_| Ok(()));

        let msgs: Vec<SessionMessageRow> = (1i64..=10)
            .map(|t| SessionMessageRow {
                id: t,
                turn_number: t,
                role: "user".to_string(),
                msg: format!("msg{t}"),
            })
            .collect();
        mock_repo
            .expect_get_messages_for_summary()
            .with(eq("ses-001"))
            .returning(move |_| Ok(msgs.clone()));

        mock_repo.expect_insert_summary().returning(|_| Ok(()));

        let mut mock_completer = MockTextCompleter::new();
        mock_completer
            .expect_complete()
            .returning(|_, _| Ok("サマリテキスト".to_string()));

        let stats = generate_summaries(&mock_repo, &mock_completer, "system", 25)
            .await
            .unwrap();

        assert_eq!(stats.sessions_processed, 1);
        assert_eq!(stats.summaries_created, 1);
    }

    #[tokio::test]
    async fn generate_summaries_multiple_chunks() {
        let mut mock_repo = MockSummaryRepository::new();
        mock_repo
            .expect_get_all_session_ids()
            .returning(|| Ok(vec!["ses-001".to_string()]));
        mock_repo
            .expect_delete_summaries_for_session()
            .with(eq("ses-001"))
            .returning(|_| Ok(()));

        // 30ターン → 2チャンク (25 + 5)
        let msgs: Vec<SessionMessageRow> = (1i64..=30)
            .map(|t| SessionMessageRow {
                id: t,
                turn_number: t,
                role: "user".to_string(),
                msg: format!("msg{t}"),
            })
            .collect();
        mock_repo
            .expect_get_messages_for_summary()
            .with(eq("ses-001"))
            .returning(move |_| Ok(msgs.clone()));

        mock_repo
            .expect_insert_summary()
            .times(2)
            .returning(|_| Ok(()));

        let mut mock_completer = MockTextCompleter::new();
        mock_completer
            .expect_complete()
            .times(2)
            .returning(|_, _| Ok("サマリ".to_string()));

        let stats = generate_summaries(&mock_repo, &mock_completer, "system", 25)
            .await
            .unwrap();

        assert_eq!(stats.sessions_processed, 1);
        assert_eq!(stats.summaries_created, 2);
    }
}
