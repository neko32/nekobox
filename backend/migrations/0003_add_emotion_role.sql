-- emotion: キャラクターの感情 (nullable: ユーザーメッセージはNULL)
ALTER TABLE session ADD COLUMN emotion VARCHAR;

-- role: メッセージ送信者の役割 (user / assistant)
ALTER TABLE session ADD COLUMN role VARCHAR NOT NULL DEFAULT 'user';

-- ── バックフィル ────────────────────────────────────────────────────────────

-- role: msg_sender_name が 'たぬ' なら user、それ以外は assistant
UPDATE session
SET role = CASE
    WHEN msg_sender_name = 'たぬ' THEN 'user'
    ELSE 'assistant'
END;

-- emotion: ユーザー(たぬ)はNULL、キャラクターはIDに基づくマッピング
UPDATE session
SET emotion = CASE
    WHEN msg_sender_name = 'たぬ' THEN NULL
    WHEN id IN (100, 166, 188)    THEN '悲しい'
    WHEN id = 98                  THEN 'びっくり'
    WHEN id IN (108, 110)         THEN '怖い'
    ELSE                               '嬉しい'
END;
