# ビルドステージ
FROM rust:latest AS builder
WORKDIR /app
COPY . .
RUN cargo build --release

# 実行ステージ（軽量）
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y libssl-dev ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/discord_task_bot /usr/local/bin/discord_task_bot
CMD ["discord_task_bot"]