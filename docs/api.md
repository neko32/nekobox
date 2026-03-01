# nekobox バックエンド API 仕様

Base URL: `http://localhost:8080`

Content-Type: `application/json` (request / response 共通)

---

## POST /v1/msg

ユーザーのメッセージをキャラクターに送り、キャラクターからの返答を受け取る。

### リクエストボディ

| フィールド | 型 | 必須 | 説明 |
|---|---|---|---|
| `character_name` | string | ✅ | キャラクター名 |
| `version` | string | ✅ | キャラクターのバージョン |
| `response_id` | string | ❌ | 会話継続用 response_id（2回目以降にセット）|
| `image_url` | string | ❌ | 画像を送る場合の URL |
| `user_name` | string | ✅ | ユーザー名 |
| `session_id` | string | ✅ | 現在のセッション ID |
| `message` | string | ✅ | キャラクターへ送るメッセージ |

**例:**

```json
{
    "character_name": "takochan",
    "version": "1.0.0",
    "response_id": null,
    "image_url": null,
    "user_name": "さのまる",
    "session_id": "na",
    "message": "初めまして、私の名前はさのまるです。自己紹介よろしくね"
}
```

### 成功レスポンス (200 OK)

| フィールド | 型 | 必須 | 説明 |
|---|---|---|---|
| `character_name` | string | ✅ | キャラクター名 |
| `version` | string | ✅ | キャラクターのバージョン |
| `response_id` | string | ❌ | 会話継続用 response_id |
| `image_url` | string | ❌ | 画像 URL |
| `user_name` | string | ✅ | ユーザー名 |
| `session_id` | string | ✅ | セッション ID |
| `message` | string | ✅ | キャラクターからのメッセージ |
| `emotion` | string | ✅ | キャラクターの感情（下記参照）|

**emotion 値一覧:**

| 値 | 意味 |
|---|---|
| `楽しい` | 楽しい |
| `嬉しい` | 嬉しい |
| `普通` | 普通（デフォルト）|
| `悲しい` | 悲しい |
| `イライラ` | イライラ |
| `うんざり` | うんざり |
| `びっくり` | びっくり |
| `怖い` | 怖い |

**例:**

```json
{
    "character_name": "takochan",
    "version": "1.0.0",
    "response_id": "chatcmpl-abc123",
    "image_url": null,
    "user_name": "さのまる",
    "session_id": "na",
    "message": "はじめまして！さのまるさん、よろしくね！",
    "emotion": "嬉しい"
}
```

### エラーレスポンス

| フィールド | 型 | 必須 | 説明 |
|---|---|---|---|
| `id` | string | ✅ | エラー識別子 |
| `message` | string | ✅ | エラー内容 |

**エラー識別子一覧:**

| id | HTTP ステータス | 説明 |
|---|---|---|
| `VALIDATION_ERROR` | 400 | リクエストのバリデーションエラー |
| `CONFIG_ERROR` | 500 | 設定ファイルのロードエラー |
| `DB_ERROR` | 500 | データベースエラー |
| `LM_STUDIO_ERROR` | 502 | LM Studio との通信エラー |
| `HTTP_REQUEST_ERROR` | 502 | HTTP リクエストエラー |

**例:**

```json
{
    "id": "VALIDATION_ERROR",
    "message": "Validation error: character_name is required"
}
```
