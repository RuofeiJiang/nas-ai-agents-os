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

## 第九步:存储与 SMB 共享

裸盘 -> 文件系统 -> 共享文件夹 -> **SMB 导出 + 授权 + SMB 密码**。
注意:建了共享文件夹 ≠ 能 SMB 访问,后面三件不做客户端连不上(踩过:共享可见但登录失败)。

```bash
NEW=$(grep OMV_CONFIGOBJECT_NEW_UUID /etc/default/openmediavault | cut -d'"' -f2)

# 1. 擦盘+建文件系统(必须串行!并发跑同一块盘会把 GPT 写坏)
omv-rpc "DiskMgmt" "wipe" '{"devicefile":"/dev/sdX","type":"quick","secure":false}'   # 异步,等 /tmp/bgstatus* 里 running:false
omv-rpc "FileSystemMgmt" "create" '{"devicefile":"/dev/disk/by-id/<wwn>","type":"ext4"}' # 异步,同上
omv-rpc "FileSystemMgmt" "setMountPoint" '{"id":"<fs-uuid>","usagewarnthreshold":85}'

# 2. 共享文件夹(uuid 必须用本机魔法 UUID,见 NEW)
omv-rpc "ShareMgmt" "set" "{\"uuid\":\"$NEW\",\"name\":\"media\",\"reldirpath\":\"media\",\"comment\":\"媒体\",\"mntentref\":\"<mountpoint-uuid>\"}"

# 3. SMB 导出(参数按服务端 schema 全量给,缺字段直接抛 SchemaValidationException)
omv-rpc "SMB" "setShare" "{\"uuid\":\"$NEW\",\"enable\":true,\"sharedfolderref\":\"<sf-uuid>\",\"comment\":\"media\",\"guest\":\"no\",\"readonly\":false,...}"
# 省力法:从 /usr/share/openmediavault/datamodels/rpc.smb.json 的 properties 按类型生成默认值再覆盖关键字段

# 4. 授权 + SMB 密码(passdb 不建,登录必失败--pdbedit -L 应显示用户)
omv-rpc "ShareMgmt" "setPrivileges" '{"uuid":"<sf-uuid>","privileges":[{"name":"<user>","perms":7,"type":"user"}]}'
omv-rpc "UserMgmt" "setUser" '{"name":"<user>","password":"<pwd>","groups":[...],"sshpubkeys":[],...}'
omv-rpc "Config" "applyChanges" '{"modules":[],"force":false}'

# 验证
smbclient -L //127.0.0.1 -U '<user>%<pwd>'   # 应列出共享
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
- [ ] `smbclient -L //127.0.0.1 -U '<user>%<pwd>'` 列出共享并能 `ls`

## 常见问题

### OMV RPC 报 Missing 'required' attribute / 值类型不符
OMV 8 的 RPC 参数按 schema 全量校验,且不同方法要求的字段比想象多(setShare 要 recyclemaxage、setUser 要 disallowusermod/sshpubkeys)。
通用解法:读 `/usr/share/openmediavault/datamodels/rpc.<service>.json` 的 properties,按类型(boolean/array/enum/integer/string)生成默认值,再覆盖业务字段。

### 新建对象报 XPath 查询失败
新建配置对象的 uuid 要用本机 `/etc/default/openmediavault` 里的 `OMV_CONFIGOBJECT_NEW_UUID`(每台机器不同),不是网上流传的固定值。

### SMB 登录失败(共享可见但拒绝访问)
`pdbedit -L` 为空 = 没建 SMB 账号,走上面第九步的 UserMgmt.setUser。

### OMV 安装失败
- 确认 Debian 版本是 trixie(`cat /etc/os-release`)
- 网络通(`ping deb.debian.org`)

### AAOS core 起不来
- `journalctl -u aaos-core -n 20` 看日志
- 确认 `/etc/aaos/env` 存在且权限 600
- 确认 `/etc/aaos/models.toml` 存在

### LLM 报错
- 确认 ARK_API_KEY 填了(设置页或 /etc/aaos/env)
- MiniMax 可选:在 `/etc/aaos/env` 设置 `MINIMAX_API_KEY`,并在 `models.toml` 使用 `provider_type = "anthropic"`、`base_url = "https://api.minimaxi.com/anthropic"`;不要把 Anthropic-compatible provider 当成 `/chat/completions`
- `aaos-cli --test-llm` 测连通性

### Web UI 不显示 AAOS
- `omv-mkworkbench all` 重生成导航
- 重启 omv-engined: `systemctl restart openmediavault-engined`
