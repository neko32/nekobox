CREATE TABLE IF NOT EXISTS session (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id          VARCHAR NOT NULL,
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
    timestamp           DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_session_session_id ON session(session_id);
