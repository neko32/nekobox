use serde::Deserialize;
use std::path::Path;

use crate::core::error::AppError;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub current_session: String,
    pub user_name: String,
    /// JSONキーは `background_image`（仕様書では `backend_image` と表記ゆれあり）
    pub background_image: Option<String>,
    pub character: CharacterConfig,
    pub model: ModelConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CharacterConfig {
    pub name: String,
    pub version: String,
    pub model_path: Option<String>,
    pub settings_path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelConfig {
    pub temperature: f32,
}

/// JSON文字列中の ${VAR} をシステム環境変数で展開する
fn expand_env_vars(s: &str) -> String {
    let mut result = s.to_string();
    for (key, value) in std::env::vars() {
        let placeholder = format!("${{{key}}}");
        result = result.replace(&placeholder, &value);
    }
    result
}

/// パスデリミタをOSに合わせて変換する（/ → \\ on Windows）
fn normalize_path(s: &str) -> String {
    if cfg!(windows) {
        s.replace('/', "\\")
    } else {
        s.to_string()
    }
}

fn expand_path(s: &str) -> String {
    normalize_path(&expand_env_vars(s))
}

impl AppConfig {
    pub fn load(cfg_path: &str) -> Result<Self, AppError> {
        let config_file = Path::new(cfg_path).join("app.config");
        let raw = std::fs::read_to_string(&config_file)
            .map_err(|e| AppError::Config(format!("Cannot read app.config: {e}")))?;

        // 環境変数を展開してからパース（パス系フィールドのみ後処理）
        let mut config: AppConfig = serde_json::from_str(&raw)
            .map_err(|e| AppError::Config(format!("Invalid app.config JSON: {e}")))?;

        // パス系フィールドを展開・正規化
        config.character.settings_path = expand_path(&config.character.settings_path);
        config.character.model_path = config.character.model_path.map(|p| expand_path(&p));
        config.background_image = config.background_image.map(|p| expand_path(&p));

        Ok(config)
    }

    /// 初回セッション（`current_session == "na"`）か
    #[must_use] 
    pub fn is_first_session(&self) -> bool {
        self.current_session == "na"
    }

    /// キャラクター設定ファイルのパスを返す: `{settings_path}/{name}_{version}.json`
    #[must_use] 
    pub fn character_settings_file(&self) -> String {
        Path::new(&self.character.settings_path)
            .join(format!(
                "{}_{}.json",
                self.character.name, self.character.version
            ))
            .to_string_lossy()
            .into_owned()
    }

    /// キャラクタープロンプトファイルのパスを返す: `{settings_path}/{name}_{version}.md`
    #[must_use] 
    pub fn character_prompt_file(&self) -> String {
        Path::new(&self.character.settings_path)
            .join(format!(
                "{}_{}.md",
                self.character.name, self.character.version
            ))
            .to_string_lossy()
            .into_owned()
    }

    /// `{name}_{version}.md` を直接読み込んで `system_prompt` として返す。
    /// ファイル内の `{{name}}` はユーザー名に置換される。
    pub fn load_system_prompt(&self) -> Result<String, AppError> {
        let path = self.character_prompt_file();
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| AppError::Config(format!("Cannot read character prompt {path}: {e}")))?;
        Ok(raw.replace("{{name}}", &self.user_name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    // ── ヘルパー ─────────────────────────────────────────────

    fn make_config_in(dir: &std::path::Path) -> AppConfig {
        AppConfig {
            current_session: "na".to_string(),
            user_name: "テスト".to_string(),
            background_image: None,
            character: CharacterConfig {
                name: "takochan".to_string(),
                version: "1.0.0".to_string(),
                model_path: None,
                settings_path: dir.to_string_lossy().into_owned(),
            },
            model: ModelConfig { temperature: 0.6 },
        }
    }

    // ── expand_env_vars ───────────────────────────────────────

    #[test]
    fn expand_env_vars_replaces_known_var() {
        std::env::set_var("NEKOBOX_TEST_VAR", "/tmp/test");
        let result = expand_env_vars("${NEKOBOX_TEST_VAR}/sub");
        assert!(result.contains("/tmp/test") || result.contains("\\tmp\\test"));
    }

    #[test]
    fn expand_env_vars_leaves_unknown_var() {
        let result = expand_env_vars("${TOTALLY_UNKNOWN_VAR_XYZ}");
        assert_eq!(result, "${TOTALLY_UNKNOWN_VAR_XYZ}");
    }

    // ── AppConfig::load ───────────────────────────────────────

    #[test]
    fn load_valid_config() {
        let tmp = tempdir().unwrap();
        let json = r#"{
            "current_session": "na",
            "user_name": "テストユーザー",
            "character": {
                "name": "takochan",
                "version": "1.0.0",
                "model_path": null,
                "settings_path": "/settings"
            },
            "model": {"temperature": 0.7}
        }"#;
        std::fs::write(tmp.path().join("app.config"), json).unwrap();

        let cfg = AppConfig::load(tmp.path().to_str().unwrap()).unwrap();
        assert_eq!(cfg.user_name, "テストユーザー");
        assert_eq!(cfg.character.name, "takochan");
        assert!((cfg.model.temperature - 0.7).abs() < 0.001);
        assert!(cfg.is_first_session());
    }

    #[test]
    fn load_missing_file_returns_err() {
        let result = AppConfig::load("/nonexistent/nekobox_test_xyz");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Cannot read"));
    }

    #[test]
    fn load_invalid_json_returns_err() {
        let tmp = tempdir().unwrap();
        std::fs::write(tmp.path().join("app.config"), "not valid json {{").unwrap();
        let result = AppConfig::load(tmp.path().to_str().unwrap());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid"));
    }

    // ── character_settings_file ───────────────────────────────

    #[test]
    fn character_settings_file_contains_name_and_version() {
        let tmp = tempdir().unwrap();
        let cfg = make_config_in(tmp.path());
        let path = cfg.character_settings_file();
        assert!(path.contains("takochan_1.0.0.json"), "パス: {path}");
    }

    // ── character_prompt_file ─────────────────────────────────

    #[test]
    fn character_prompt_file_contains_name_and_version() {
        let tmp = tempdir().unwrap();
        let cfg = make_config_in(tmp.path());
        let path = cfg.character_prompt_file();
        assert!(path.contains("takochan_1.0.0.md"), "パス: {path}");
    }

    // ── load_system_prompt ────────────────────────────────────

    #[test]
    fn load_system_prompt_valid() {
        let tmp = tempdir().unwrap();
        std::fs::write(
            tmp.path().join("takochan_1.0.0.md"),
            "あなたはたこちゃんです\n\n# キャラ設定\nかわいい",
        )
        .unwrap();

        let cfg = make_config_in(tmp.path());
        let prompt = cfg.load_system_prompt().unwrap();
        assert!(prompt.contains("あなたはたこちゃんです"));
    }

    #[test]
    fn load_system_prompt_replaces_name_placeholder() {
        let tmp = tempdir().unwrap();
        std::fs::write(
            tmp.path().join("takochan_1.0.0.md"),
            "こんにちは、{{name}}さん！",
        )
        .unwrap();

        let cfg = make_config_in(tmp.path());
        let prompt = cfg.load_system_prompt().unwrap();
        assert_eq!(prompt, "こんにちは、テストさん！");
    }

    #[test]
    fn load_system_prompt_missing_file_returns_err() {
        let cfg = make_config_in(std::path::Path::new("/nonexistent/nekobox_test_xyz"));
        let err = cfg.load_system_prompt().unwrap_err();
        assert!(err.to_string().contains("Cannot read"));
    }

    // ── is_first_session ─────────────────────────────────────

    #[test]
    fn is_first_session_true_when_na() {
        let tmp = tempdir().unwrap();
        let cfg = make_config_in(tmp.path());
        assert!(cfg.is_first_session());
    }

    #[test]
    fn is_first_session_false_when_uuid() {
        let tmp = tempdir().unwrap();
        let mut cfg = make_config_in(tmp.path());
        cfg.current_session = "some-uuid-1234".to_string();
        assert!(!cfg.is_first_session());
    }
}
