use async_trait::async_trait;
use chrono::Utc;
use sqlx::{Row, SqlitePool};

use crate::core::{error::AppError, models::SessionLog};

/// 会話ログ永続化のトレイト（モック・スタブ差し替え可能）
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait ConversationRepository: Send + Sync {
    async fn save_log(&self, log: &SessionLog) -> Result<(), AppError>;
    async fn get_logs_by_session(&self, session_id: &str) -> Result<Vec<SessionLog>, AppError>;
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
                model_instance_id, input_tokens, total_output_tokens, timestamp
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_logs_by_session(&self, session_id: &str) -> Result<Vec<SessionLog>, AppError> {
        let rows = sqlx::query(
            r"
            SELECT session_id, session_alias, background_image, msg_sender_name, user_name,
                   settings_name, msg, image_url, response_id,
                   model_instance_id, input_tokens, total_output_tokens, timestamp
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
                timestamp           DATETIME NOT NULL
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
            msg_sender_name: "user".to_string(),
            user_name: "さのまる".to_string(),
            settings_name: "takochan_1.0.0".to_string(),
            msg: msg.to_string(),
            image_url: None,
            response_id: None,
            model_instance_id: None,
            input_tokens: None,
            total_output_tokens: None,
            timestamp: Utc::now(),
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
        };

        repo.save_log(&log).await.unwrap();

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM session")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
    }
}
