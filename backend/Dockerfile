# ─────────────────────────────────────────────────────────────────────────────
#  nekobox backend — マルチステージビルド (Debian Bookworm Slim ランタイム)
# ─────────────────────────────────────────────────────────────────────────────

# ── ステージ 1: ビルド ────────────────────────────────────────────────────────
FROM rust:bookworm AS builder

WORKDIR /app

# rust:bookworm には gcc が含まれているため追加インストール不要
# sqlx bundled: SQLite を C ソースからコンパイルして静的リンク
# reqwest rustls-tls: OpenSSL 不要の純 Rust TLS

# 依存クレートを先にキャッシュ（ソース変更時に再ビルドしない）
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src && \
    echo 'fn main() {}' > src/main.rs && \
    echo '' > src/lib.rs
RUN cargo build --release --bin nekobox-backend
RUN rm -f src/main.rs src/lib.rs

# 実ソースをコピーして本番バイナリをビルド
COPY src       ./src
COPY migrations ./migrations
# タイムスタンプを更新してリビルドをトリガー
RUN touch src/main.rs src/lib.rs
RUN cargo build --release --bin nekobox-backend

# ── ステージ 2: ランタイム ────────────────────────────────────────────────────
FROM debian:bookworm-slim

WORKDIR /app

# uv バイナリを公式イメージからコピー
COPY --from=ghcr.io/astral-sh/uv:latest /uv /usr/local/bin/uv

# SQLite は静的リンク済み・TLS は rustls (純 Rust) のため ca-certificates のみ必要
# jq は entrypoint.sh で mcp_servers.json をパースするために必要
RUN apt-get update && apt-get install -y \
        ca-certificates \
        jq \
    && rm -rf /var/lib/apt/lists/*

# 非 root ユーザーで実行（セキュリティベストプラクティス）
# ディレクトリを root として作成・chown してから USER を切り替える。
# こうしないと bind mount 初回マウント時にディレクトリが root 所有となり
# nekobox ユーザーが書き込めない。
# /nekobox/uv : uv ツールの永続化ディレクトリ (tools/ + bin/)
RUN useradd -m -u 1000 -s /bin/sh nekobox && \
    mkdir -p /nekobox/config /nekobox/data /nekobox/uv/tools /nekobox/uv/bin && \
    chown -R nekobox:nekobox /nekobox

USER nekobox

COPY --from=builder /app/target/release/nekobox-backend /app/nekobox-backend
COPY entrypoint.sh /app/entrypoint.sh

# ─── ボリューム ──────────────────────────────────────────────────────────────
# /nekobox/config : app.config・キャラクター設定・背景画像をマウント (読み取り専用)
# /nekobox/data   : SQLite DB ファイル出力先をマウント
# /nekobox/uv     : uv ツールキャッシュをマウント (コンテナ再作成後も再インストール不要)
VOLUME ["/nekobox/config", "/nekobox/data", "/nekobox/uv"]

# ─── デフォルト環境変数 ──────────────────────────────────────────────────────
# コンテナ内では全インターフェースにバインドして、ホスト側から 8080 に転送する
# UV_TOOL_DIR / UV_TOOL_BIN_DIR : uv ツールを /nekobox/uv 以下に集約
ENV NEKOBOX_BIND_HOST=0.0.0.0 \
    NEKOBOX_CFG_PATH=/nekobox/config \
    NEKOBOX_DB_PATH=/nekobox/data \
    RUST_LOG=info \
    UV_TOOL_DIR=/nekobox/uv/tools \
    UV_TOOL_BIN_DIR=/nekobox/uv/bin \
    PATH="/nekobox/uv/bin:/usr/local/bin:${PATH}"

EXPOSE 8080

ENTRYPOINT ["sh", "/app/entrypoint.sh"]
