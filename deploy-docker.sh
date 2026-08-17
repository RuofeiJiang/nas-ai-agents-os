#!/bin/bash
#
# AAOS-NAS Docker 部署脚本
# 在装好 OMV 8 的宿主机上执行
#
set -e

echo "=========================================="
echo "  AAOS-NAS Docker 部署"
echo "=========================================="

# 1. 准备配置目录
echo ">>> 1. 准备配置"
mkdir -p config
cp models.toml agents.json kb-context.json kb-tasks.json kb-nas.json config/

# 如果没有 .env,从示例复制
if [ ! -f .env ]; then
    cp docker.env.example .env
    echo ">>> 已创建 .env,请编辑填入 API Key:"
    echo "    nano .env"
    echo "    填完再运行: docker compose up -d"
    exit 0
fi

# 2. 构建 + 启动
echo ">>> 2. 构建镜像"
docker compose build 2>&1 | tail -5

echo ">>> 3. 启动服务"
docker compose up -d 2>&1 | tail -10

# 3. 等待启动
echo ">>> 4. 等待启动..."
sleep 5

# 4. 验证
echo ">>> 5. 验证"
docker compose ps

echo ""
echo "=== 服务状态 ==="
docker exec aaos-core aaos-cli --test-llm 2>&1 | head -3 || echo "core 未就绪(检查 ARK_API_KEY)"

echo ""
echo "=========================================="
echo "  部署完成!"
echo ""
echo "  CLI: docker exec aaos-core aaos-cli 系统信息"
echo "  日志: docker logs -f aaos-core"
echo "  巡检: docker exec aaos-core aaos-cli --daily-check"
echo ""
echo "  Web UI 需单独部署 OMV 插件:"
echo "  dpkg -i omv-plugin/openmediavault-aaos_*.deb"
echo "=========================================="
