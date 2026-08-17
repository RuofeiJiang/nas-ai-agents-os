#!/bin/bash
#
# AAOS-NAS 一键部署脚本
# 在装好 OMV 8 的 Debian trixie 上执行
# 用法: bash deploy-aaos.sh
#
set -e

AAOS_DIR="~/nas-ai-agents-os"
OMV_AAOS_DIR="~/nas-ai-agents-os/omv-plugin"
VM_IP="${1:-192.168.122.57}"  # 默认 VM,真机改成真机 IP

echo "=========================================="
echo "  AAOS-NAS 部署脚本"
echo "  目标: ${VM_IP}"
echo "=========================================="

# ========== 1. 构建 ==========
echo ">>> 1. 构建 Rust 二进制(musl 静态)"
cd "${AAOS_DIR}"
. "$HOME/.cargo/env" 2>/dev/null
cargo build --release --target x86_64-unknown-linux-musl 2>&1 | tail -2
echo "    二进制: $(ls -lh target/x86_64-unknown-linux-musl/release/aaos-* | wc -l) 个"

echo ">>> 2. 构建 OMV 插件 DEB"
cd "${OMV_AAOS_DIR}"
dpkg-buildpackage -us -uc -b 2>&1 | tail -2
echo "    DEB: $(ls -lh ${AAOS_DIR}/openmediavault-aaos_*.deb | awk '{print $5}')"

echo ">>> 3. 构建 Angular workbench"
cd "${OMV_AAOS_DIR}/workbench"
npx ng build --configuration production 2>&1 | tail -2
cd dist && tar czf /tmp/aaos-workbench.tar.gz openmediavault-workbench
echo "    workbench: $(ls -lh /tmp/aaos-workbench.tar.gz | awk '{print $5}')"

# ========== 2. 部署到目标 ==========
echo ">>> 4. 部署到 ${VM_IP}"

# 创建远程目录
ssh -o StrictHostKeyChecking=no root@${VM_IP} 'mkdir -p /etc/aaos /var/lib/aaos /var/log/aaos'

# Rust 二进制
scp -o StrictHostKeyChecking=no \
    ${AAOS_DIR}/target/x86_64-unknown-linux-musl/release/aaos-core \
    ${AAOS_DIR}/target/x86_64-unknown-linux-musl/release/aaos-sentinel \
    ${AAOS_DIR}/target/x86_64-unknown-linux-musl/release/aaos-cli \
    root@${VM_IP}:/usr/local/bin/
ssh root@${VM_IP} 'chmod +x /usr/local/bin/aaos-*'

# 配置文件
scp -o StrictHostKeyChecking=no \
    ${AAOS_DIR}/models.toml \
    ${AAOS_DIR}/agents.json \
    ${AAOS_DIR}/kb-context.json \
    ${AAOS_DIR}/kb-tasks.json \
    ${AAOS_DIR}/kb-nas.json \
    root@${VM_IP}:/etc/aaos/

# env 文件(如果远程没有)
ssh root@${VM_IP} 'test -f /etc/aaos/env || echo "# AAOS 环境变量(部署后填)" > /etc/aaos/env; chmod 600 /etc/aaos/env'

# systemd 服务
ssh root@${VM_IP} 'cat > /etc/systemd/system/aaos-sentinel.service << "EOF"
[Unit]
Description=AAOS Sentinel - Audit Logger
After=network.target

[Service]
Type=simple
ExecStart=/usr/local/bin/aaos-sentinel --socket /run/aaos/sentinel.sock --log-file /var/log/aaos/sentinel.log
Restart=on-failure
RestartSec=5
RuntimeDirectory=aaos
RuntimeDirectoryMode=0755
User=root

[Install]
WantedBy=multi-user.target
EOF

cat > /etc/systemd/system/aaos-core.service << "EOF"
[Unit]
Description=AAOS Core Agent
After=network.target openmediavault-engined.service
Requires=aaos-sentinel.service
After=aaos-sentinel.service

[Service]
Type=simple
EnvironmentFile=/etc/aaos/env
ExecStart=/usr/local/bin/aaos-core --socket /run/aaos/core.sock --sentinel-socket /run/aaos/sentinel.sock
Restart=on-failure
RestartSec=5
User=root

[Install]
WantedBy=multi-user.target
EOF

cat > /etc/systemd/system/aaos-daily-check.service << "EOF"
[Unit]
Description=AAOS Daily System Check
After=network-online.target

[Service]
Type=oneshot
EnvironmentFile=/etc/aaos/env
ExecStart=/usr/local/bin/aaos-cli --daily-check
StandardOutput=journal
StandardError=journal
EOF

cat > /etc/systemd/system/aaos-daily-check.timer << "EOF"
[Unit]
Description=AAOS Daily Check Timer

[Timer]
OnCalendar=*-*-* 08:00:00
Persistent=true
RandomizedDelaySec=300

[Install]
WantedBy=timers.target
EOF

systemctl daemon-reload
systemctl enable aaos-sentinel aaos-core aaos-daily-check.timer
'

# OMV 插件 DEB
scp -o StrictHostKeyChecking=no \
    ${AAOS_DIR}/openmediavault-aaos_0.1.0-1_all.deb \
    root@${VM_IP}:/tmp/
ssh root@${VM_IP} 'dpkg -i /tmp/openmediavault-aaos_0.1.0-1_all.deb 2>&1 | tail -2 || true; apt-get install -f -y 2>&1 | tail -2'

# Angular workbench
scp -o StrictHostKeyChecking=no /tmp/aaos-workbench.tar.gz root@${VM_IP}:/tmp/
ssh root@${VM_IP} 'cd /var/www/openmediavault && tar xzf /tmp/aaos-workbench.tar.gz --strip-components=1'

# 重启服务
ssh root@${VM_IP} 'systemctl restart openmediavault-engined; sleep 2; systemctl restart aaos-sentinel; sleep 1; systemctl restart aaos-core; sleep 2; omv-mkworkbench all 2>/dev/null || true'

# ========== 3. 验证 ==========
echo ">>> 5. 验证"
ssh root@${VM_IP} '
echo "=== 服务状态 ==="
systemctl is-active aaos-sentinel aaos-core openmediavault-engined

echo "=== AAOS RPC ==="
omv-rpc "AAOS" "status" "{}" 2>&1

echo "=== 快速测试 ==="
set -a; source /etc/aaos/env; set +a
aaos-cli 系统信息 2>&1 | head -c 100
echo ""

echo "=== Web UI ==="
curl -sI http://localhost/ 2>&1 | head -1
echo ""
echo "=========================================="
echo "  部署完成!"
echo "  Web UI: http://$(hostname -I | awk "{print \$1}")/"
echo "  登录: admin / <你的OMV密码>"
echo "  菜单: Services > AAOS"
echo ""
echo "  下一步:"
echo "  1. 浏览器开 Web UI -> Services > AAOS -> 设置"
echo "  2. 填 ARK_API_KEY(火山 coding plan)"
echo "  3. 填 HA_TOKEN(若装了 HA)"
echo "  4. 改 kb-context.json(真机硬件/目录/服务)"
echo "  5. 装磁盘 -> 建文件系统 -> 建共享"
echo "  6. 验证: aaos-cli 系统信息(秒回)"
echo "=========================================="
'
