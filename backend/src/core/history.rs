use std::collections::VecDeque;

/// OpenAI-compat Chat Completion API における短期記憶バッファ。
///
/// 最大 `max_turns` ターン分のメッセージを保持し、上限を超えた場合は
/// 最も古いターンを自動削除する（FIFO デキュー方式）。
///
/// 型パラメーター `T` によって任意のメッセージ型と組み合わせて利用できる
/// ため、他システム・他クレートからも再利用可能なライブラリコンポーネント
/// として設計されている。
///
/// # ターンの定義
///
/// 1ターン = `[user メッセージ, (tool メッセージ)*, assistant メッセージ]`
/// のように、1回の往復に属するメッセージ群の `Vec<T>` として扱う。
pub struct MessageHistory<T: Clone> {
    turns: VecDeque<Vec<T>>,
    max_turns: usize,
    session_id: String,
}

impl<T: Clone> MessageHistory<T> {
    /// 新しい `MessageHistory` を生成する。
    ///
    /// # Arguments
    /// * `max_turns` - 保持する最大ターン数（0 の場合は push_turn が何もしない）
    /// * `session_id` - 紐づくセッションID
    #[must_use]
    pub fn new(max_turns: usize, session_id: impl Into<String>) -> Self {
        Self {
            turns: VecDeque::new(),
            max_turns,
            session_id: session_id.into(),
        }
    }

    /// ターンを追加する。
    ///
    /// 追加後にターン数が `max_turns` を超えた場合、最も古いターンを削除する。
    /// `max_turns` が 0 の場合は何もしない。
    pub fn push_turn(&mut self, msgs: Vec<T>) {
        if self.max_turns == 0 {
            return;
        }
        self.turns.push_back(msgs);
        while self.turns.len() > self.max_turns {
            self.turns.pop_front();
        }
    }

    /// 保持している全ターンのメッセージをフラット化して返す。
    ///
    /// ターンの追加順（古い→新しい）で並ぶ。
    #[must_use]
    pub fn to_messages(&self) -> Vec<T> {
        self.turns.iter().flatten().cloned().collect()
    }

    /// 現在のセッションID を返す。
    #[must_use]
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// ヒストリをリセットして新しいセッションに切り替える。
    ///
    /// `/new` コマンドによるセッション切り替え時に呼び出す。
    /// `initial_turns` には DB から復元したターン群を古い順に渡す。
    pub fn reset(&mut self, session_id: impl Into<String>, initial_turns: Vec<Vec<T>>) {
        self.session_id = session_id.into();
        self.turns.clear();
        for turn in initial_turns {
            self.push_turn(turn);
        }
    }

    /// 保持しているターン数を返す。
    #[must_use]
    pub fn len(&self) -> usize {
        self.turns.len()
    }

    /// ターンが一つも保持されていない場合に `true` を返す。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.turns.is_empty()
    }
}

// ───────────────────────────────────── テスト ──────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn msgs(strs: &[&str]) -> Vec<String> {
        strs.iter().map(|s| (*s).to_string()).collect()
    }

    // ── 基本操作 ──────────────────────────────────────────────────────────

    #[test]
    fn push_turn_adds_messages() {
        let mut h: MessageHistory<String> = MessageHistory::new(25, "ses-1");
        h.push_turn(msgs(&["user: hi", "assistant: hello"]));
        assert_eq!(h.len(), 1);
        assert_eq!(h.to_messages(), msgs(&["user: hi", "assistant: hello"]));
    }

    #[test]
    fn to_messages_flattens_all_turns() {
        let mut h: MessageHistory<String> = MessageHistory::new(25, "ses-1");
        h.push_turn(msgs(&["u1", "a1"]));
        h.push_turn(msgs(&["u2", "a2"]));
        assert_eq!(h.to_messages(), msgs(&["u1", "a1", "u2", "a2"]));
    }

    #[test]
    fn to_messages_empty_when_no_turns() {
        let h: MessageHistory<String> = MessageHistory::new(25, "ses-1");
        assert!(h.to_messages().is_empty());
    }

    // ── 上限管理 ──────────────────────────────────────────────────────────

    #[test]
    fn exactly_max_turns_no_trim() {
        let mut h: MessageHistory<String> = MessageHistory::new(25, "ses-1");
        for i in 0..25 {
            h.push_turn(msgs(&[&format!("u{i}"), &format!("a{i}")]));
        }
        assert_eq!(h.len(), 25);
        // 最初のターンがまだ残っている
        assert_eq!(h.to_messages()[0], "u0");
    }

    #[test]
    fn push_turn_trims_when_over_max() {
        let mut h: MessageHistory<String> = MessageHistory::new(3, "ses-1");
        h.push_turn(msgs(&["u1", "a1"]));
        h.push_turn(msgs(&["u2", "a2"]));
        h.push_turn(msgs(&["u3", "a3"]));
        h.push_turn(msgs(&["u4", "a4"])); // ← 4ターン目でu1が消える
        assert_eq!(h.len(), 3);
        let flat = h.to_messages();
        assert!(!flat.contains(&"u1".to_string()), "u1 は削除されているべき");
        assert!(flat.contains(&"u2".to_string()));
        assert!(flat.contains(&"u4".to_string()));
    }

    #[test]
    fn push_turn_26th_removes_first() {
        let mut h: MessageHistory<String> = MessageHistory::new(25, "ses-1");
        for i in 0..26 {
            h.push_turn(msgs(&[&format!("u{i}"), &format!("a{i}")]));
        }
        assert_eq!(h.len(), 25);
        let flat = h.to_messages();
        // u0 は削除されている
        assert!(!flat.contains(&"u0".to_string()));
        // u1 〜 u25 が残っている
        assert!(flat.contains(&"u1".to_string()));
        assert!(flat.contains(&"u25".to_string()));
    }

    #[test]
    fn max_turns_zero_does_nothing() {
        let mut h: MessageHistory<String> = MessageHistory::new(0, "ses-1");
        h.push_turn(msgs(&["u1", "a1"]));
        assert_eq!(h.len(), 0);
        assert!(h.is_empty());
    }

    // ── セッション管理 ──────────────────────────────────────────────────────

    #[test]
    fn session_id_returns_initial_id() {
        let h: MessageHistory<String> = MessageHistory::new(25, "ses-abc");
        assert_eq!(h.session_id(), "ses-abc");
    }

    #[test]
    fn reset_clears_turns_and_changes_session() {
        let mut h: MessageHistory<String> = MessageHistory::new(25, "ses-old");
        h.push_turn(msgs(&["u1", "a1"]));
        h.push_turn(msgs(&["u2", "a2"]));
        assert_eq!(h.len(), 2);

        h.reset("ses-new".to_string(), vec![]);
        assert_eq!(h.session_id(), "ses-new");
        assert!(h.is_empty());
    }

    #[test]
    fn reset_with_initial_turns_populates_history() {
        let mut h: MessageHistory<String> = MessageHistory::new(25, "ses-old");
        h.push_turn(msgs(&["u1", "a1"])); // 古いデータ

        let initial = vec![
            msgs(&["restored_u1", "restored_a1"]),
            msgs(&["restored_u2", "restored_a2"]),
        ];
        h.reset("ses-new".to_string(), initial);

        assert_eq!(h.session_id(), "ses-new");
        assert_eq!(h.len(), 2);
        let flat = h.to_messages();
        assert!(flat.contains(&"restored_u1".to_string()));
        assert!(flat.contains(&"restored_u2".to_string()));
        assert!(!flat.contains(&"u1".to_string()));
    }

    #[test]
    fn reset_respects_max_turns() {
        let mut h: MessageHistory<String> = MessageHistory::new(2, "ses-old");
        // 3ターン分を initial_turns として渡すと max_turns=2 に収まる
        let initial = vec![
            msgs(&["u1", "a1"]),
            msgs(&["u2", "a2"]),
            msgs(&["u3", "a3"]),
        ];
        h.reset("ses-new".to_string(), initial);
        assert_eq!(h.len(), 2);
        let flat = h.to_messages();
        assert!(
            !flat.contains(&"u1".to_string()),
            "u1 は max_turns 超過で削除"
        );
        assert!(flat.contains(&"u2".to_string()));
        assert!(flat.contains(&"u3".to_string()));
    }

    // ── is_empty / len ────────────────────────────────────────────────────

    #[test]
    fn is_empty_true_on_new() {
        let h: MessageHistory<String> = MessageHistory::new(25, "ses-1");
        assert!(h.is_empty());
        assert_eq!(h.len(), 0);
    }

    #[test]
    fn is_empty_false_after_push() {
        let mut h: MessageHistory<String> = MessageHistory::new(25, "ses-1");
        h.push_turn(msgs(&["u1", "a1"]));
        assert!(!h.is_empty());
        assert_eq!(h.len(), 1);
    }

    // ── ツールメッセージ混在ターン ─────────────────────────────────────────

    #[test]
    fn push_turn_with_tool_messages() {
        let mut h: MessageHistory<String> = MessageHistory::new(25, "ses-1");
        // 1ターン = user + tool + assistant
        h.push_turn(msgs(&[
            "user: 天気は？",
            "tool: 晴れ",
            "assistant: 晴れですよ！",
        ]));
        let flat = h.to_messages();
        assert_eq!(flat.len(), 3);
        assert_eq!(flat[1], "tool: 晴れ");
    }
}
