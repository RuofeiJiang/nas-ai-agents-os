#!/usr/bin/env bash
# AAOS 安装脚本
# 在 TrueNAS Scale 上以 root 身份运行

set -euo pipefail

PREFIX="${PREFIX:-/usr/local}"
SERVICE_DIR="${SERVICE_DIR:-/etc/systemd/system}"
LOG_DIR="${LOG_DIR:-/var/log/aaos}"
RUN_DIR="${RUN_DIR:-/run/aaos}"

# 1. 创建目录
mkdir -p "$LOG_DIR"

# 2. 构建（必须已经 rustup）
echo "[1/4] cargo build --release..."
cargo build --release

# 3. 安装二进制
echo "[2/4] install binaries to $PREFIX/bin..."
install -m 0755 target/release/aaos-core "$PREFIX/bin/aaos-core"
install -m 0755 target/release/aaos-sentinel "$PREFIX/bin/aaos-sentinel"
install -m 0755 target/release/aaos-cli "$PREFIX/bin/aaos-cli"

# 4. 安装 systemd unit
echo "[3/4] install systemd units..."
install -m 0644 systemd/aaos-core.service "$SERVICE_DIR/aaos-core.service"
install -m 0644 systemd/aaos-sentinel.service "$SERVICE_DIR/aaos-sentinel.service"

# 5. 重载 + 启动
echo "[4/4] systemctl daemon-reload + enable + start..."
systemctl daemon-reload
systemctl enable aaos-sentinel.service aaos-core.service
systemctl restart aaos-sentinel.service
systemctl restart aaos-core.service

echo ""
echo "安装完成。"
echo "  journalctl -u aaos-core -f"
echo "  journalctl -u aaos-sentinel -f"
echo "  tail -f $LOG_DIR/sentinel.log"
echo "  aaos-cli '列出池'"
