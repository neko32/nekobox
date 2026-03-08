-- turn_number: 会話のターン番号（1始まり、userメッセージごとにインクリメント）
ALTER TABLE session ADD COLUMN turn_number INTEGER NOT NULL DEFAULT 0;

-- ── バックフィル ────────────────────────────────────────────────────────────
-- 各行のturn_numberを「同じsession_id内でid以下のuserロール行の累積件数」に設定する。
-- idはAUTOINCREMENTのため挿入順と一致し、メッセージの時系列順序と等価。
--
-- 例: user(id=1)→assistant(id=2)→user(id=3)→tool(id=4)→assistant(id=5) の場合
--   id=1: users with id<=1 = 1 → turn 1
--   id=2: users with id<=2 = 1 → turn 1 (同じターン)
--   id=3: users with id<=3 = 2 → turn 2
--   id=4: users with id<=4 = 2 → turn 2 (同じターン)
--   id=5: users with id<=5 = 2 → turn 2 (同じターン)
UPDATE session
SET turn_number = (
    SELECT COUNT(*)
    FROM session s2
    WHERE s2.session_id = session.session_id
      AND s2.role = 'user'
      AND s2.id <= session.id
);
