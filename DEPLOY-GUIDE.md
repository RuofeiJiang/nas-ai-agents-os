# AAOS-NAS 真机部署指南

## 前置条件

- [x] Debian 13.6.0 ISO U 盘已就绪
- [x] 部署脚本 `deploy-aaos.sh` 已就绪
- [ ] 真机硬件(PC/小主机/纳米 PC,有 USB 接口)
- [ ] 硬盘(系统盘 + 数据盘)

## 第一步:装 Debian 13 trixie

1. U 盘插入真机,BIOS 选 U 盘启动
2. 安装时选:
   - **不装桌面环境**(最小化安装)
   - **磁盘分区**:系统盘(ext4,/) + 数据盘(先不格式化,OMV 里建)
   - **用户**:设 root 密码 + 普通用户
   - **网络**:DHCP 或固定 IP(记下 IP)
3. 装完重启,拔 U 盘

## 第二步:装 OMV 8

```bash
# SSH 进真机
ssh root@<真机IP>

# 装 OMV
wget -O - https://get.openmediavault.io | sh

# 等 10-15 分钟,装完自动重启 omv-engined
```

## 第三步:配置 OMV 基础

浏览器开 `http://<真机IP>/`,登录 `admin` / `<你的密码>`:
1. 改 admin 密码(System > General Settings)
2. 装 ZFS 插件(System > Plugins > 搜 zfs)
3. 装 podman 插件(System > Plugins > 搜 podman)
4. 格式化数据盘(Storage > Disks > 格式化 ext4/btrfs/zfs)
5. 挂载数据盘(Storage > File Systems > Mount)
6. 建 1-2 个共享文件夹(Storage > Shared Folders)

## 第四步:部署 AAOS

在**开发机**(当前这台,有源码的)上执行:

```bash
cd ~/nas-ai-agents-os

# 部署到真机(改 IP)
bash deploy-aaos.sh <真机IP>
```

脚本自动完成:
- 构建 Rust 二进制(musl 静态)
- 构建 OMV 插件 DEB
- 构建 Angular workbench
- scp 全部到真机
- 装 systemd 服务 + 开机自启
- 装 OMV 插件
- 部署 workbench
- 启动 + 验证

## 第五步:配置 AAOS

浏览器开 `http://<真机IP>/`,Services > AAOS,点 **⚙️ 设置**:

1. **ARK_API_KEY**: 填火山 coding plan 的 API key
   ```
   <你的 ARK_API_KEY>
   ```
2. **HA_TOKEN**(若装了 HA): 填 HA long-lived token
3. 保存 -> core 自动重启

## 第六步:按真机配置 kb-context.json

SSH 进真机,编辑 `/etc/aaos/kb-context.json`:

```json
{
  "nas_name": "我的NAS",
  "hardware": {
    "cpu": "实际CPU型号",
    "cores": 实际核数,
    "ram_gb": 实际内存,
    "disks": [
      {"device": "/dev/sda", "size": "实际大小", "role": "系统盘"},
      {"device": "/dev/sdb", "size": "实际大小", "role": "数据盘(ZFS)"}
    ],
    "gpu": "实际GPU(如有)"
  },
  "directories": {
    "shared_folders": [
      {"name": "实际共享名", "device": "实际设备", "fs": "ext4/zfs", "purpose": "照片/媒体/下载..."}
    ],
    "backups": "/usb(外接盘)"
  },
  "docker_services": [
    {"name": "homeassistant", "purpose": "智能家居", "status": "待部署"},
    {"name": "jellyfin", "purpose": "影音", "status": "待部署"}
  ],
  "backup_strategy": {
    "method": "rsync 增量",
    "target": "/usb",
    "schedule": "每周日 02:00"
  }
}
```

或用 OMV web UI 的 AAOS 设置页改 models.toml。

## 第七步(可选):装 HA

```bash
# 在真机上
mkdir -p /srv/ha-config
podman run -d --name homeassistant --privileged --network=host \
  -v /srv/ha-config:/config -e TZ=Asia/Shanghai \
  --restart=unless-stopped \
  docker.io/homeassistant/home-assistant:latest

# 配自启
podman generate systemd --name homeassistant > /etc/systemd/system/homeassistant.service
systemctl enable homeassistant
```

浏览器开 `http://<真机IP>:8123/`,建账号,生成 token,填进 AAOS 设置。

## 第八步(可选):装 qBittorrent

```bash
apt install -y qbittorrent-nox ffmpeg
# 配 QBIT_HOST/QBIT_USER/QBIT_PASS 到 /etc/aaos/env
```

## 验证清单

- [ ] `aaos-cli 系统信息` 秒回(快速回复)
- [ ] `aaos-cli 列出磁盘` 显示真机磁盘
- [ ] Web UI Services > AAOS 聊天面板能用
- [ ] ⚙️ 设置能改 key
- [ ] 快捷按钮(系统信息/列出磁盘/列出共享)能点
- [ ] `aaos-cli 创建共享叫test` 多步 Plan 通
- [ ] `aaos-cli 分析系统有没有异常` LLM 智能分析通
- [ ] 重启真机 -> core/sentinel 自启
- [ ] 每日巡检 timer active

## 常见问题

### OMV 安装失败
- 确认 Debian 版本是 trixie(`cat /etc/os-release`)
- 网络通(`ping deb.debian.org`)

### AAOS core 起不来
- `journalctl -u aaos-core -n 20` 看日志
- 确认 `/etc/aaos/env` 存在且权限 600
- 确认 `/etc/aaos/models.toml` 存在

### LLM 报错
- 确认 ARK_API_KEY 填了(设置页或 /etc/aaos/env)
- `aaos-cli --test-llm` 测连通性

### Web UI 不显示 AAOS
- `omv-mkworkbench all` 重生成导航
- 重启 omv-engined: `systemctl restart openmediavault-engined`
