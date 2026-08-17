# ===== Stage 1: Build Rust (musl 静态) =====
FROM rust:latest AS builder

# 装 musl 工具链
RUN apt-get update && apt-get install -y musl-tools
RUN rustup target add x86_64-unknown-linux-musl

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src/ ./src/

# 构建
RUN cargo build --release --target x86_64-unknown-linux-musl

# ===== Stage 2: 运行时(最小镜像) =====
FROM alpine:latest

# 装最小运行时依赖
RUN apk add --no-cache ca-certificates curl podman

# 拷贝二进制
COPY --from=builder /build/target/x86_64-unknown-linux-musl/release/aaos-core /usr/local/bin/
COPY --from=builder /build/target/x86_64-unknown-linux-musl/release/aaos-sentinel /usr/local/bin/
COPY --from=builder /build/target/x86_64-unknown-linux-musl/release/aaos-cli /usr/local/bin/
RUN chmod +x /usr/local/bin/aaos-*

# 拷贝配置(默认值,实际用 volume 挂载覆盖)
COPY models.toml agents.json kb-context.json kb-tasks.json kb-nas.json /etc/aaos/
COPY systemd/ /etc/aaos/systemd/

# 运行时目录
RUN mkdir -p /var/lib/aaos /var/log/aaos /run/aaos

# 环境变量(运行时填)
ENV ARK_API_KEY=""
ENV HA_TOKEN=""
ENV ALIST_URL=""
ENV ALIST_USER=""
ENV ALIST_PASS=""

# 健康检查
HEALTHCHECK --interval=30s --timeout=5s --retries=3 \
  CMD aaos-cli --test-llm 2>/dev/null || exit 1

# 默认启动 core(可被 compose 覆盖)
CMD ["aaos-core", "--socket", "/run/aaos/core.sock", "--sentinel-socket", "/run/aaos/sentinel.sock"]
