use async_trait::async_trait;
use chrono::Utc;
use sqlx::{Row, SqlitePool};

use crate::core::{
    error::AppError,
    models::{Role, SessionLog},
};

/// 会話ログ永続化のトレイト（モック・スタブ差し替え可能）
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait ConversationRepository: Send + Sync {
    async fn save_log(&self, log: &SessionLog) -> Result<(), AppError>;
    async fn get_logs_by_session(&self, session_id: &str) -> Result<Vec<SessionLog>, AppError>;
    /// `session_id` に対応する現在の最大 `turn_number` を返す。レコード無しなら 0。
    async fn get_current_turn(&self, session_id: &str) -> Result<i64, AppError>;
    /// `session_id` の直近 `max_turns` ターン分のログを古い順に返す。
    ///
    /// 例えば `max_turns=25` を指定すると、turn_number の降順で上位 25 件の
    /// distinct turn_number に属するログを、turn_number ASC・timestamp ASC で返す。
    async fn get_recent_turns(
        &self,
        session_id: &str,
        max_turns: i64,
    ) -> Result<Vec<SessionLog>, AppError>;
}

pub struct SqliteConversationRepository {
    pool: SqlitePool,
}

impl SqliteConversationRepository {
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ConversationRepository for SqliteConversationRepository {
    async fn save_log(&self, log: &SessionLog) -> Result<(), AppError> {
        let timestamp = log.timestamp.to_rfc3339();
        sqlx::query(
            r"
            INSERT INTO session (
                session_id, session_alias, background_image, msg_sender_name, user_name,
                settings_name, msg, image_url, response_id,
                model_instance_id, input_tokens, total_output_tokens, timestamp,
                role, emotion, turn_number
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ",
        )
        .bind(&log.session_id)
        .bind(&log.session_alias)
        .bind(&log.background_image)
        .bind(&log.msg_sender_name)
        .bind(&log.user_name)
        .bind(&log.settings_name)
        .bind(&log.msg)
        .bind(&log.image_url)
        .bind(&log.response_id)
        .bind(&log.model_instance_id)
        .bind(log.input_tokens)
        .bind(log.total_output_tokens)
        .bind(&timestamp)
        .bind(log.role.as_str())
        .bind(&log.emotion)
        .bind(log.turn_number)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_logs_by_session(&self, session_id: &str) -> Result<Vec<SessionLog>, AppError> {
        let rows = sqlx::query(
            r"
            SELECT session_id, session_alias, background_image, msg_sender_name, user_name,
                   settings_name, msg, image_url, response_id,
                   model_instance_id, input_tokens, total_output_tokens, timestamp,
                   role, emotion, turn_number
            FROM session
            WHERE session_id = ?
            ORDER BY timestamp ASC
            ",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|r| {
                let ts_str: String = r.get("timestamp");
                let timestamp = chrono::DateTime::parse_from_rfc3339(&ts_str)
                    .map_err(|_| AppError::Config("Invalid timestamp format".into()))?
                    .with_timezone(&Utc);
                let role_str: String = r.get("role");
                let role = Role::from_str(&role_str)
                    .ok_or_else(|| AppError::Config(format!("Invalid role value: {role_str}")))?;
                Ok(SessionLog {
                    session_id: r.get("session_id"),
                    session_alias: r.get("session_alias"),
                    background_image: r.get("background_image"),
                    msg_sender_name: r.get("msg_sender_name"),
                    user_name: r.get("user_name"),
                    settings_name: r.get("settings_name"),
                    msg: r.get("msg"),
                    image_url: r.get("image_url"),
                    response_id: r.get("response_id"),
                    model_instance_id: r.get("model_instance_id"),
                    input_tokens: r.get("input_tokens"),
                    total_output_tokens: r.get("total_output_tokens"),
                    timestamp,
                    role,
                    emotion: r.get("emotion"),
                    turn_number: r.get("turn_number"),
                })
            })
            .collect()
    }

    async fn get_current_turn(&self, session_id: &str) -> Result<i64, AppError> {
        let turn: Option<i64> =
            sqlx::query_scalar("SELECT MAX(turn_number) FROM session WHERE session_id = ?")
                .bind(session_id)
                .fetch_one(&self.pool)
                .await?;
        Ok(turn.unwrap_or(0))
    }

    async fn get_recent_turns(
        &self,
        session_id: &str,
        max_turns: i64,
    ) -> Result<Vec<SessionLog>, AppError> {
        let rows = sqlx::query(
            r"
            SELECT session_id, session_alias, background_image, msg_sender_name, user_name,
                   settings_name, msg, image_url, response_id,
                   model_instance_id, input_tokens, total_output_tokens, timestamp,
                   role, emotion, turn_number
            FROM session
            WHERE session_id = ?
              AND turn_number IN (
                  SELECT DISTINCT turn_number
                  FROM session
                  WHERE session_id = ?
                  ORDER BY turn_number DESC
                  LIMIT ?
              )
            ORDER BY turn_number ASC, timestamp ASC
            ",
        )
        .bind(session_id)
        .bind(session_id)
        .bind(max_turns)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|r| {
                let ts_str: String = r.get("timestamp");
                let timestamp = chrono::DateTime::parse_from_rfc3339(&ts_str)
                    .map_err(|_| AppError::Config("Invalid timestamp format".into()))?
                    .with_timezone(&Utc);
                let role_str: String = r.get("role");
                let role = Role::from_str(&role_str)
                    .ok_or_else(|| AppError::Config(format!("Invalid role value: {role_str}")))?;
                Ok(SessionLog {
                    session_id: r.get("session_id"),
                    session_alias: r.get("session_alias"),
                    background_image: r.get("background_image"),
                    msg_sender_name: r.get("msg_sender_name"),
                    user_name: r.get("user_name"),
                    settings_name: r.get("settings_name"),
                    msg: r.get("msg"),
                    image_url: r.get("image_url"),
                    response_id: r.get("response_id"),
                    model_instance_id: r.get("model_instance_id"),
                    input_tokens: r.get("input_tokens"),
                    total_output_tokens: r.get("total_output_tokens"),
                    timestamp,
                    role,
                    emotion: r.get("emotion"),
                    turn_number: r.get("turn_number"),
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    async fn in_memory_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query(
            r"CREATE TABLE session (
                id                  INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id          VARCHAR NOT NULL,
                session_alias       VARCHAR,
                background_image    VARCHAR NOT NULL,
                msg_sender_name     VARCHAR NOT NULL,
                user_name           VARCHAR NOT NULL,
                settings_name       VARCHAR NOT NULL,
                msg                 VARCHAR NOT NULL,
                image_url           VARCHAR,
                response_id         VARCHAR,
                model_instance_id   VARCHAR,
                input_tokens        INTEGER,
                total_output_tokens INTEGER,
                timestamp           DATETIME NOT NULL,
                role                VARCHAR NOT NULL DEFAULT 'user',
                emotion             VARCHAR,
                turn_number         INTEGER NOT NULL DEFAULT 0
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    fn sample_log(session_id: &str, msg: &str) -> SessionLog {
        SessionLog {
            session_id: session_id.to_string(),
            session_alias: None,
            background_image: "/bg.png".to_string(),
            msg_sender_name: "たぬ".to_string(),
            user_name: "たぬ".to_string(),
            settings_name: "takochan_1.0.0".to_string(),
            msg: msg.to_string(),
            image_url: None,
            response_id: None,
            model_instance_id: None,
            input_tokens: None,
            total_output_tokens: None,
            timestamp: Utc::now(),
            role: Role::User,
            emotion: None,
            turn_number: 1,
        }
    }

    #[tokio::test]
    async fn save_log_inserts_row() {
        let pool = in_memory_pool().await;
        let repo = SqliteConversationRepository::new(pool.clone());

        repo.save_log(&sample_log("ses-001", "こんにちは"))
            .await
            .unwrap();

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM session")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn save_log_multiple_rows() {
        let pool = in_memory_pool().await;
        let repo = SqliteConversationRepository::new(pool.clone());

        repo.save_log(&sample_log("ses-001", "メッセージ1"))
            .await
            .unwrap();
        repo.save_log(&sample_log("ses-001", "メッセージ2"))
            .await
            .unwrap();
        repo.save_log(&sample_log("ses-002", "メッセージ3"))
            .await
            .unwrap();

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM session")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 3);
    }

    #[tokio::test]
    async fn save_and_get_role_and_emotion() {
        let pool = in_memory_pool().await;
        let repo = SqliteConversationRepository::new(pool.clone());

        let user_log = sample_log("ses-010", "こんにちは");
        repo.save_log(&user_log).await.unwrap();

        let assistant_log = SessionLog {
            session_id: "ses-010".to_string(),
            session_alias: None,
            background_image: "/bg.png".to_string(),
            msg_sender_name: "takochan".to_string(),
            user_name: "たぬ".to_string(),
            settings_name: "takochan_1.0.0".to_string(),
            msg: "やあ！".to_string(),
            image_url: None,
            response_id: None,
            model_instance_id: None,
            input_tokens: None,
            total_output_tokens: None,
            timestamp: Utc::now(),
            role: Role::Assistant,
            emotion: Some("嬉しい".to_string()),
            turn_number: 1,
        };
        repo.save_log(&assistant_log).await.unwrap();

        let logs = repo.get_logs_by_session("ses-010").await.unwrap();
        assert_eq!(logs.len(), 2);
        assert_eq!(logs[0].role, Role::User);
        assert!(logs[0].emotion.is_none());
        assert_eq!(logs[1].role, Role::Assistant);
        assert_eq!(logs[1].emotion.as_deref(), Some("嬉しい"));
    }

    #[tokio::test]
    async fn save_log_with_optional_fields() {
        let pool = in_memory_pool().await;
        let repo = SqliteConversationRepository::new(pool.clone());

        let log = SessionLog {
            session_id: "ses-003".to_string(),
            session_alias: None,
            background_image: "/bg.png".to_string(),
            msg_sender_name: "takochan".to_string(),
            user_name: "さのまる".to_string(),
            settings_name: "takochan_1.0.0".to_string(),
            msg: "やあ！".to_string(),
            image_url: Some("http://example.com/img.png".to_string()),
            response_id: Some("resp-abc".to_string()),
            model_instance_id: Some("model-xyz".to_string()),
            input_tokens: Some(50),
            total_output_tokens: Some(30),
            timestamp: Utc::now(),
            role: Role::Assistant,
            emotion: Some("嬉しい".to_string()),
            turn_number: 1,
        };

        repo.save_log(&log).await.unwrap();

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM session")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
    }

    // ── get_current_turn のテスト (ぐんまちゃんT1〜T6) ──────────────────────

    /// T1: セッション新規 → get_current_turn が 0 を返す
    #[tokio::test]
    async fn get_current_turn_returns_zero_for_new_session() {
        let pool = in_memory_pool().await;
        let repo = SqliteConversationRepository::new(pool);

        let turn = repo.get_current_turn("ses-new").await.unwrap();
        assert_eq!(turn, 0);
    }

    /// T2: user+assistant 保存後 → 両者が turn_number=1、get_current_turn=1
    #[tokio::test]
    async fn get_current_turn_returns_one_after_first_exchange() {
        let pool = in_memory_pool().await;
        let repo = SqliteConversationRepository::new(pool);

        let user_log = SessionLog {
            turn_number: 1,
            role: Role::User,
            ..sample_log("ses-t2", "こんにちは")
        };
        let assistant_log = SessionLog {
            turn_number: 1,
            role: Role::Assistant,
            msg_sender_name: "takochan".to_string(),
            ..sample_log("ses-t2", "やあ！")
        };

        repo.save_log(&user_log).await.unwrap();
        repo.save_log(&assistant_log).await.unwrap();

        let turn = repo.get_current_turn("ses-t2").await.unwrap();
        assert_eq!(turn, 1);

        let logs = repo.get_logs_by_session("ses-t2").await.unwrap();
        assert_eq!(logs[0].turn_number, 1);
        assert_eq!(logs[1].turn_number, 1);
    }

    /// T3: 2ターン目 → 2回目のuserが turn_number=2
    #[tokio::test]
    async fn get_current_turn_increments_for_second_exchange() {
        let pool = in_memory_pool().await;
        let repo = SqliteConversationRepository::new(pool);

        repo.save_log(&SessionLog {
            turn_number: 1,
            role: Role::User,
            ..sample_log("ses-t3", "1回目")
        })
        .await
        .unwrap();
        repo.save_log(&SessionLog {
            turn_number: 1,
            role: Role::Assistant,
            ..sample_log("ses-t3", "返答1")
        })
        .await
        .unwrap();
        repo.save_log(&SessionLog {
            turn_number: 2,
            role: Role::User,
            ..sample_log("ses-t3", "2回目")
        })
        .await
        .unwrap();
        repo.save_log(&SessionLog {
            turn_number: 2,
            role: Role::Assistant,
            ..sample_log("ses-t3", "返答2")
        })
        .await
        .unwrap();

        let turn = repo.get_current_turn("ses-t3").await.unwrap();
        assert_eq!(turn, 2);

        let logs = repo.get_logs_by_session("ses-t3").await.unwrap();
        assert_eq!(logs[2].turn_number, 2);
        assert_eq!(logs[3].turn_number, 2);
    }

    /// T4: userが連続2回（パターン3: 返事喪失）→ 2件目userが turn_number=2
    #[tokio::test]
    async fn consecutive_user_messages_get_different_turns() {
        let pool = in_memory_pool().await;
        let repo = SqliteConversationRepository::new(pool);

        repo.save_log(&SessionLog {
            turn_number: 1,
            role: Role::User,
            ..sample_log("ses-t4", "1回目")
        })
        .await
        .unwrap();
        repo.save_log(&SessionLog {
            turn_number: 2,
            role: Role::User,
            ..sample_log("ses-t4", "2回目")
        })
        .await
        .unwrap();

        let turn = repo.get_current_turn("ses-t4").await.unwrap();
        assert_eq!(turn, 2);

        let logs = repo.get_logs_by_session("ses-t4").await.unwrap();
        assert_eq!(logs[0].turn_number, 1);
        assert_eq!(logs[1].turn_number, 2);
    }

    // ── get_recent_turns のテスト ────────────────────────────────────────

    /// RT1: セッションなし → 空リスト
    #[tokio::test]
    async fn get_recent_turns_empty_session_returns_empty() {
        let pool = in_memory_pool().await;
        let repo = SqliteConversationRepository::new(pool);
        let logs = repo.get_recent_turns("ses-empty", 25).await.unwrap();
        assert!(logs.is_empty());
    }

    /// RT2: 3ターン存在・max_turns=25 → 全ターン返す
    #[tokio::test]
    async fn get_recent_turns_returns_all_when_under_max() {
        let pool = in_memory_pool().await;
        let repo = SqliteConversationRepository::new(pool);

        for t in 1i64..=3 {
            repo.save_log(&SessionLog {
                turn_number: t,
                role: Role::User,
                ..sample_log("ses-rt2", &format!("user msg {t}"))
            })
            .await
            .unwrap();
            repo.save_log(&SessionLog {
                turn_number: t,
                role: Role::Assistant,
                msg_sender_name: "takochan".to_string(),
                ..sample_log("ses-rt2", &format!("assistant msg {t}"))
            })
            .await
            .unwrap();
        }

        let logs = repo.get_recent_turns("ses-rt2", 25).await.unwrap();
        assert_eq!(logs.len(), 6); // 3ターン × 2メッセージ
        assert_eq!(logs[0].turn_number, 1);
        assert_eq!(logs[4].turn_number, 3);
    }

    /// RT3: 30ターン存在・max_turns=25 → 直近25ターンのみ
    #[tokio::test]
    async fn get_recent_turns_returns_only_newest_n_turns() {
        let pool = in_memory_pool().await;
        let repo = SqliteConversationRepository::new(pool);

        for t in 1i64..=30 {
            repo.save_log(&SessionLog {
                turn_number: t,
                role: Role::User,
                ..sample_log("ses-rt3", &format!("user {t}"))
            })
            .await
            .unwrap();
            repo.save_log(&SessionLog {
                turn_number: t,
                role: Role::Assistant,
                msg_sender_name: "takochan".to_string(),
                ..sample_log("ses-rt3", &format!("assistant {t}"))
            })
            .await
            .unwrap();
        }

        let logs = repo.get_recent_turns("ses-rt3", 25).await.unwrap();
        // 直近25ターン（turn 6〜30）× 2メッセージ = 50件
        assert_eq!(logs.len(), 50);
        // 最初の要素は turn_number=6
        assert_eq!(logs[0].turn_number, 6);
        // 最後の要素は turn_number=30
        assert_eq!(logs[49].turn_number, 30);
        // turn_number=1〜5 は含まれていない
        assert!(logs.iter().all(|l| l.turn_number >= 6));
    }

    /// RT4: max_turns=0 → 空リスト
    #[tokio::test]
    async fn get_recent_turns_max_zero_returns_empty() {
        let pool = in_memory_pool().await;
        let repo = SqliteConversationRepository::new(pool);

        repo.save_log(&sample_log("ses-rt4", "msg")).await.unwrap();

        let logs = repo.get_recent_turns("ses-rt4", 0).await.unwrap();
        assert!(logs.is_empty());
    }

    /// RT5: 複数セッション混在でも対象セッションのみ返す
    #[tokio::test]
    async fn get_recent_turns_isolated_per_session() {
        let pool = in_memory_pool().await;
        let repo = SqliteConversationRepository::new(pool);

        repo.save_log(&SessionLog {
            session_id: "ses-a".into(),
            turn_number: 1,
            role: Role::User,
            ..sample_log("ses-a", "ses-a msg")
        })
        .await
        .unwrap();
        repo.save_log(&SessionLog {
            session_id: "ses-b".into(),
            turn_number: 1,
            role: Role::User,
            ..sample_log("ses-b", "ses-b msg")
        })
        .await
        .unwrap();

        let logs_a = repo.get_recent_turns("ses-a", 25).await.unwrap();
        assert_eq!(logs_a.len(), 1);
        assert_eq!(logs_a[0].msg, "ses-a msg");

        let logs_b = repo.get_recent_turns("ses-b", 25).await.unwrap();
        assert_eq!(logs_b.len(), 1);
        assert_eq!(logs_b[0].msg, "ses-b msg");
    }

    /// RT6: ターン内の順序（timestamp ASC）が保たれる
    #[tokio::test]
    async fn get_recent_turns_orders_by_turn_then_timestamp() {
        let pool = in_memory_pool().await;
        let repo = SqliteConversationRepository::new(pool);

        // ターン1: user → assistant
        repo.save_log(&SessionLog {
            turn_number: 1,
            role: Role::User,
            ..sample_log("ses-rt6", "ターン1-user")
        })
        .await
        .unwrap();
        repo.save_log(&SessionLog {
            turn_number: 1,
            role: Role::Assistant,
            msg_sender_name: "takochan".to_string(),
            ..sample_log("ses-rt6", "ターン1-assistant")
        })
        .await
        .unwrap();
        // ターン2: user → assistant
        repo.save_log(&SessionLog {
            turn_number: 2,
            role: Role::User,
            ..sample_log("ses-rt6", "ターン2-user")
        })
        .await
        .unwrap();
        repo.save_log(&SessionLog {
            turn_number: 2,
            role: Role::Assistant,
            msg_sender_name: "takochan".to_string(),
            ..sample_log("ses-rt6", "ターン2-assistant")
        })
        .await
        .unwrap();

        let logs = repo.get_recent_turns("ses-rt6", 25).await.unwrap();
        assert_eq!(logs.len(), 4);
        assert_eq!(logs[0].turn_number, 1);
        assert_eq!(logs[0].role, Role::User);
        assert_eq!(logs[1].turn_number, 1);
        assert_eq!(logs[1].role, Role::Assistant);
        assert_eq!(logs[2].turn_number, 2);
        assert_eq!(logs[3].turn_number, 2);
    }

    /// T6: 複数セッション分離 → 別セッションのターン番号が混在しない
    #[tokio::test]
    async fn turn_numbers_are_isolated_per_session() {
        let pool = in_memory_pool().await;
        let repo = SqliteConversationRepository::new(pool);

        // セッションA: 3ターン
        for t in 1i64..=3 {
            repo.save_log(&SessionLog {
                session_id: "ses-a".into(),
                turn_number: t,
                role: Role::User,
                ..sample_log("ses-a", "msg")
            })
            .await
            .unwrap();
        }
        // セッションB: 1ターン
        repo.save_log(&SessionLog {
            session_id: "ses-b".into(),
            turn_number: 1,
            role: Role::User,
            ..sample_log("ses-b", "msg")
        })
        .await
        .unwrap();

        assert_eq!(repo.get_current_turn("ses-a").await.unwrap(), 3);
        assert_eq!(repo.get_current_turn("ses-b").await.unwrap(), 1);
    }
}
