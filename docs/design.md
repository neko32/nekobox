# nekobox 設計ドキュメント

## 概要

nekobox は Godot (C#) フロントエンドと Rust (Axum/Tokio) バックエンドで構成される
Windows 向けデスクトップコンパニオンアプリケーションです。
ローカルで動作する LM Studio を経由してAIキャラクターとリアルタイムに会話します。

## アーキテクチャ

```
[ユーザー]
    ↓ テキスト入力 (モードレスウィンドウ)
[Godot フロントエンド (C#)]
    ↓ HTTP POST /v1/msg
[Rust バックエンド (Axum/Tokio) — Docker コンテナ]
    ├── SQLite3 (会話ログ保存)
    └── LM Studio API (/v1/chat/completions)
    ↓ { message, emotion }
[Godot フロントエンド] — キャラクター表示・絵文字
```

## コンポーネント詳細

### フロントエンド (frontend/)

| 要素 | 詳細 |
|------|------|
| エンジン | Godot 4.6.1 (Mono/C#) |
| 言語 | C# (.NET 8) |
| 画面サイズ | 1200px (固定) |
| ベースカラー | #7297c5 |
| セカンダリカラー | #666666 |

**主要シーン構成**

- `Main.tscn` — ルートシーン。環境変数チェック・config ロード・起動フローを管理
- `CompanionView.tscn` — キャラクターレンダリング・背景表示（ブラー付き）
- `ChatWindow.tscn` — 半透明会話チャットウィンドウ（画面下部）
- `InputPopup.tscn` — モードレスのテキスト入力ウィンドウ（Enter 送信・Shift+Enter 改行）

**起動フロー**

1. 必須環境変数チェック (`NEKOBOX_CFG_PATH`, `NEKOBOX_DB_PATH`, `NEKOBOX_LMSTUDIO_HOST`, `NEKOBOX_LMSTUDIO_PORT`)
2. `app.config` ロード (JSON)
3. 背景画像ロード・ブラー適用
4. 2D/3D キャラクターモデルロード・レンダリング
5. `current_session == "na"` の場合は初回メッセージを `/v1/msg` に送信
6. そうでなければ「おかえり」メッセージを送信
7. レスポンス受信後、`emotion` に合わせた絵文字とともに会話ウィンドウに表示

**感情→絵文字マッピング**

| emotion | 絵文字 |
|---------|--------|
| 楽しい  | 😄 |
| 嬉しい  | 😊 |
| 普通    | 😐 |
| 悲しい  | 😢 |
| イライラ | 😠 |
| うんざり | 😩 |
| びっくり | 😲 |
| 怖い    | 😨 |

### バックエンド (backend/)

| 要素 | 詳細 |
|------|------|
| 言語 | Rust (Edition 2021) |
| Webフレームワーク | Axum 0.8 |
| 非同期ランタイム | Tokio |
| DB | SQLite 3 (sqlx 0.8) |
| HTTP クライアント | reqwest 0.12 |
| コンテナ | Docker (Debian Slim) |

**モジュール構成**

```
src/
├── main.rs                  # エントリポイント・DI組み立て
├── api/
│   ├── lm_studio.rs         # LmStudioClient trait + HttpLmStudioClient
│   └── routes/
│       └── msg.rs           # POST /v1/msg ハンドラ
└── core/
    ├── config.rs            # AppConfig ローダー
    ├── db.rs                # ConversationRepository trait + SQLite実装
    ├── error.rs             # AppError (IntoResponse 実装)
    └── models.rs            # ドメインモデル (new type パターン)
```

**設計方針**

- 外部依存 (`LmStudioClient`, `ConversationRepository`) はすべて trait 抽象化してモック可能
- データバリデーションは new type パターン (`SessionId`, `UserName`, `CharacterName` など)
- エラーは `AppError` に集約して HTTP レスポンスへ自動変換

### データベース (SQLite3)

DB ファイル: `$NEKOBOX_DB_PATH/nekobox.sqlite3`

**session テーブルスキーマ**

| カラム | 型 | 説明 |
|--------|-----|------|
| id | INTEGER PK AUTOINCREMENT | |
| session_id | VARCHAR | セッション UUID |
| background_image | VARCHAR | 会話時の背景画像パス |
| msg_sender_name | VARCHAR | 送信者名（ユーザーまたはキャラ）|
| user_name | VARCHAR | ユーザー名 |
| settings_name | VARCHAR | キャラ設定ファイル名 |
| msg | VARCHAR | メッセージ本文 |
| image_url | VARCHAR? | 画像 URL |
| response_id | VARCHAR? | LM Studio の conversation ID |
| model_instance_id | VARCHAR? | モデルインスタンス名 |
| input_tokens | INTEGER? | プロンプトトークン数 |
| total_output_tokens | INTEGER? | 出力トークン数 |
| timestamp | DATETIME | 記録日時 |

## 設定ファイル

### app.config (JSON)

```json
{
    "current_session": "na",
    "user_name": "ユーザー名",
    "background_image": "${NEKOBOX_CFG_PATH}/image/bg.png",
    "character": {
        "name": "takochan",
        "version": "1.0.0",
        "model_path": "${NEKOBOX_CFG_PATH}/character/model",
        "settings_path": "${NEKOBOX_CFG_PATH}/character"
    },
    "model": {
        "temperature": 0.6
    }
}
```

> **注**: `background_image` キーは仕様書サンプルでは `backend_image` と記載されているが
> これは誤記と判断し `background_image` を正とする。

### キャラクター設定ファイル

パス: `{settings_path}/{name}_{version}.json`

```json
{
    "system_prompt": "あなたは ○○ です。以下の指示に従ってください...\n回答は必ず JSON で返してください。形式: {\"message\": \"...\", \"emotion\": \"普通\"}"
}
```

## 環境変数

| 変数名 | 必須 | 説明 |
|--------|------|------|
| `NEKOBOX_CFG_PATH` | ✅ | app.config が置かれたディレクトリ |
| `NEKOBOX_DB_PATH` | ✅ | SQLite DB が置かれたディレクトリ |
| `NEKOBOX_LMSTUDIO_HOST` | ✅ | LM Studio ホスト |
| `NEKOBOX_LMSTUDIO_PORT` | ✅ | LM Studio ポート |
| `RUST_LOG` | ❌ | ログレベル (既定: info) |

## MVP 以降の拡張予定

- キャラクターの入れ替え機能
- `emotion` に合わせたキャラクターアニメーション
