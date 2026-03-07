use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// New type: セッションID
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionId(String);

impl SessionId {
    pub fn new(id: impl Into<String>) -> Result<Self, String> {
        let id = id.into();
        if id.is_empty() {
            return Err("Session ID cannot be empty".into());
        }
        Ok(Self(id))
    }

    #[must_use] 
    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    #[must_use] 
    pub fn initial() -> Self {
        Self("na".into())
    }

    #[must_use] 
    pub fn is_na(&self) -> bool {
        self.0 == "na"
    }

    #[must_use] 
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// New type: ユーザー名
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserName(String);

impl UserName {
    pub fn new(name: impl Into<String>) -> Result<Self, String> {
        let name = name.into();
        if name.is_empty() {
            return Err("User name cannot be empty".into());
        }
        Ok(Self(name))
    }

    #[must_use] 
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// New type: キャラクター名
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterName(String);

impl CharacterName {
    pub fn new(name: impl Into<String>) -> Result<Self, String> {
        let name = name.into();
        if name.is_empty() {
            return Err("Character name cannot be empty".into());
        }
        Ok(Self(name))
    }

    #[must_use] 
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// New type: キャラクターバージョン
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterVersion(String);

impl CharacterVersion {
    pub fn new(version: impl Into<String>) -> Result<Self, String> {
        let version = version.into();
        if version.is_empty() {
            return Err("Character version cannot be empty".into());
        }
        Ok(Self(version))
    }

    #[must_use] 
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// キャラクターの感情
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum Emotion {
    #[serde(rename = "楽しい")]
    Fun,
    #[serde(rename = "嬉しい")]
    Happy,
    #[default]
    #[serde(rename = "普通")]
    Neutral,
    #[serde(rename = "悲しい")]
    Sad,
    #[serde(rename = "イライラ")]
    Irritated,
    #[serde(rename = "うんざり")]
    FedUp,
    #[serde(rename = "びっくり")]
    Surprised,
    #[serde(rename = "怖い")]
    Scared,
}

impl Emotion {
    #[must_use] 
    pub fn as_str(&self) -> &str {
        match self {
            Self::Fun => "楽しい",
            Self::Happy => "嬉しい",
            Self::Neutral => "普通",
            Self::Sad => "悲しい",
            Self::Irritated => "イライラ",
            Self::FedUp => "うんざり",
            Self::Surprised => "びっくり",
            Self::Scared => "怖い",
        }
    }

    #[allow(clippy::should_implement_trait)]
    #[must_use] 
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "楽しい" => Some(Self::Fun),
            "嬉しい" => Some(Self::Happy),
            "普通" => Some(Self::Neutral),
            "悲しい" => Some(Self::Sad),
            "イライラ" => Some(Self::Irritated),
            "うんざり" => Some(Self::FedUp),
            "びっくり" => Some(Self::Surprised),
            "怖い" => Some(Self::Scared),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── SessionId ────────────────────────────────────────────

    #[test]
    fn session_id_new_valid() {
        let id = SessionId::new("abc-123").unwrap();
        assert_eq!(id.as_str(), "abc-123");
    }

    #[test]
    fn session_id_new_empty_returns_err() {
        assert!(SessionId::new("").is_err());
    }

    #[test]
    fn session_id_generate_is_not_empty_and_not_na() {
        let id = SessionId::generate();
        assert!(!id.as_str().is_empty());
        assert!(!id.is_na());
    }

    #[test]
    fn session_id_initial_is_na() {
        let id = SessionId::initial();
        assert!(id.is_na());
        assert_eq!(id.as_str(), "na");
    }

    #[test]
    fn session_id_non_na_is_not_na() {
        let id = SessionId::new("some-uuid").unwrap();
        assert!(!id.is_na());
    }

    // ── UserName ─────────────────────────────────────────────

    #[test]
    fn user_name_new_valid() {
        let name = UserName::new("さのまる").unwrap();
        assert_eq!(name.as_str(), "さのまる");
    }

    #[test]
    fn user_name_new_empty_returns_err() {
        assert!(UserName::new("").is_err());
    }

    // ── CharacterName ─────────────────────────────────────────

    #[test]
    fn character_name_new_valid() {
        let name = CharacterName::new("takochan").unwrap();
        assert_eq!(name.as_str(), "takochan");
    }

    #[test]
    fn character_name_new_empty_returns_err() {
        assert!(CharacterName::new("").is_err());
    }

    // ── CharacterVersion ──────────────────────────────────────

    #[test]
    fn character_version_new_valid() {
        let ver = CharacterVersion::new("1.0.0").unwrap();
        assert_eq!(ver.as_str(), "1.0.0");
    }

    #[test]
    fn character_version_new_empty_returns_err() {
        assert!(CharacterVersion::new("").is_err());
    }

    // ── Emotion ───────────────────────────────────────────────

    #[test]
    fn emotion_from_str_all_valid() {
        let cases = [
            ("楽しい", "楽しい"),
            ("嬉しい", "嬉しい"),
            ("普通", "普通"),
            ("悲しい", "悲しい"),
            ("イライラ", "イライラ"),
            ("うんざり", "うんざり"),
            ("びっくり", "びっくり"),
            ("怖い", "怖い"),
        ];
        for (input, expected) in cases {
            let emotion = Emotion::from_str(input).unwrap();
            assert_eq!(emotion.as_str(), expected, "感情: {input}");
        }
    }

    #[test]
    fn emotion_from_str_unknown_returns_none() {
        assert!(Emotion::from_str("不明な感情").is_none());
    }

    #[test]
    fn emotion_default_is_neutral() {
        assert_eq!(Emotion::default().as_str(), "普通");
    }
}

/// 会話ログ（sessionテーブル用）
#[derive(Debug, Clone)]
pub struct SessionLog {
    pub session_id: String,
    pub session_alias: Option<String>,
    pub background_image: String,
    pub msg_sender_name: String,
    pub user_name: String,
    pub settings_name: String,
    pub msg: String,
    pub image_url: Option<String>,
    pub response_id: Option<String>,
    pub model_instance_id: Option<String>,
    pub input_tokens: Option<i64>,
    pub total_output_tokens: Option<i64>,
    pub timestamp: DateTime<Utc>,
}
