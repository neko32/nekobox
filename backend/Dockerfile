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

# SQLite は静的リンク済み・TLS は rustls (純 Rust) のため ca-certificates のみ必要
RUN apt-get update && apt-get install -y \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# 非 root ユーザーで実行（セキュリティベストプラクティス）
RUN useradd -m -u 1000 -s /bin/sh nekobox
USER nekobox

COPY --from=builder /app/target/release/nekobox-backend /app/nekobox-backend

# ─── ボリューム ──────────────────────────────────────────────────────────────
# /nekobox/config : app.config・キャラクター設定・背景画像をマウント (読み取り専用)
# /nekobox/data   : SQLite DB ファイル出力先をマウント
VOLUME ["/nekobox/config", "/nekobox/data"]

# ─── デフォルト環境変数 ──────────────────────────────────────────────────────
# コンテナ内では全インターフェースにバインドして、ホスト側から 8080 に転送する
ENV NEKOBOX_BIND_HOST=0.0.0.0 \
    NEKOBOX_CFG_PATH=/nekobox/config \
    NEKOBOX_DB_PATH=/nekobox/data \
    RUST_LOG=info

EXPOSE 8080

ENTRYPOINT ["/app/nekobox-backend"]
