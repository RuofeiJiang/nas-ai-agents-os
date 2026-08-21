# AAOS-NAS 项目文档

> AI Agents Operating System for Network Attached Storage
> 基于 OpenMediaVault 8 的 AI 原生 NAS 系统

---

## 1. 项目概述

AAOS-NAS 将 AAOS(AI Agents Operating System)架构植入 OpenMediaVault 8,实现 AI 作为原生系统服务管理 NAS 全部功能。用户用自然语言与 NAS 交互,Core Agent 理解意图、调度 Execution Agent 执行,结果翻译回自然语言。

### 核心论点(来自 AAOS 论文)
- LLM 是知识库,不是 agent;agent 由架构定义
- Core Agent 负责调度(开放边界),Execution Agent 负责执行(已知边界)
- CLI 集成层是 OS 与系统的分界线
- 知识库驱动调度,不过度依赖 prompt

### 技术栈
| 层 | 技术 |
|---|---|
| 底座 | OpenMediaVault 8.5.5-1(Debian 13 trixie) |
| Core Agent | Rust(musl 静态二进制) |
| LLM | deepseek-v4-pro(火山方舟 coding plan,OpenAI 兼容) |
| Execution Agent | Rust(8 个,预设执行) |
| OMV 集成 | PHP RPC 插件 + Angular 聊天面板 |
| IPC | Unix Socket(JSON line 协议) |
| 容器 | podman(OMV 插件) |
| 智能家居 | HomeAssistant(podman 容器) |

---

## 2. 架构

```
用户自然语言
  │
  ▼
Core Agent(LLM: deepseek-v4-pro)
  │  L1 系统上下文(kb-context.json,工具查:明确目标/对象)
  │  L2 查询知识架构(KB优先,LLM推理,必要时网络搜索)
  │  L3 系统实时状态(system-agent/check_state,工具查)
  │  调度模式(kb-tasks.json,工具查)
  │
  ├── 查知识库(4 个工具)
  │   ├── list_agents        查 agent + action 清单
  │   ├── get_system_context 查 NAS 配置(L2)
  │   ├── check_state        查实时状态(L3)
  │   └── get_task_pattern   查任务调度模式
  │
  ├── 输出调度决策(快速语言)
  │   ├── Intent{agent, action, args}    单步
  │   ├── Plan{steps: [Intent, ...]}     多步
  │   └── 自然语言                          预检不过/直接回复
  │
  ▼
Execution Agent(8 个,预设执行)
  │  接 Intent -> 查域知识库 -> 执行 -> 返回 ActionResult
  │
  ▼
OMV(omv-rpc)/ CLI(podman/rsync/ffmpeg)/ 模型(vision)
  │
  ▼
ActionResult{success, data, message}
  │
  ▼
Core Agent: ActionResult -> 自然语言 -> 用户

Sentinel: 全程审计日志(独立进程)
每日巡检: systemd timer 08:00 -> NL 报告
```

### 三阶段任务预检

| 阶段 | 名称 | 作用 | 典型来源 |
|---|---|---|---|
| L1 | 系统上下文 | 明确用户目标、任务对象和本机相关配置 | `kb-context.json` / `get_system_context` |
| L2 | 查询知识架构 | 优先查询确定性 KB；不足时使用 LLM 推理，必要时联网搜索并保留来源 | agent KB / `search_knowledge` / web |
| L3 | 系统实时状态 | 确认磁盘、容器、SMART、内存等当前是否具备执行条件 | `system-agent/check_state` |

> 这里的 L1/L2/L3 是**执行前预检阶段**，不是知识可信度分级。知识来源的优先级是 KB（确定性知识）→ LLM（可能有幻觉的模型知识）→ 网络（带来源的外部动态知识）。

### 调度模式知识库(kb-tasks.json)
复杂任务分解模式,Core 用 get_task_pattern 工具查询,不靠 prompt 教怎么拆任务。

---

## 3. Core Agent

### 职责
1. 明确目标与对象(L1 系统上下文)
2. 查询知识架构(L2: KB 优先,必要时 LLM/网络)
3. 输出调度决策(Intent/Plan,快速语言)
4. 预检:L3 查实时状态,判断能否执行;不过则直接回复用户
5. 翻译:ActionResult -> 自然语言返回用户

### 模型配置
```toml
# /etc/aaos/models.toml
[[providers]]
id = "coding-plan"
name = "火山方舟 Coding Plan"
provider_type = "openai"
base_url = "https://ark.cn-beijing.volces.com/api/coding/v3"
api_key_env = "ARK_API_KEY"

[core]
provider = "coding-plan"
model = "deepseek-v4-pro-260425"
fallback_provider = "local-ollama"
fallback_model = "qwen2.5:7b"
```

### 调度流程
```
用户 NL
  -> Core 查 KB(list_agents + get_system_context + check_state + get_task_pattern)
  -> 输出:
     Intent JSON {"agent":"...","action":"...","args":{}}    -> 单步
     Plan JSON   {"steps":[{agent,action,args}, ...]}        -> 多步
     自然语言                                               -> 预检不过/直接回复
  -> 破坏性检查(action 含 delete/destroy/remove/wipe/prune)
     -> 需确认: 返回 NeedsConfirmation + token
     -> 用户 --confirm <token> 后执行
  -> 架构 dispatch -> agent 执行 -> ActionResult
  -> Core 翻译 ActionResult -> NL -> 用户
```

---

## 4. Execution Agent 全量功能(9 个 / 77 个 action)

### 4.1 system-agent(12 个)
系统维护:SMART/系统信息/磁盘/文件系统/用户/空间/实时状态/配置/缓存/更新/网络/重启

| # | action | 说明 | 参数 | 破坏性 |
|---|---|---|---|---|
| 1 | check_smart | 磁盘 SMART 健康(含温度) | - | |
| 2 | system_info | CPU/内存/uptime/版本 | - | |
| 3 | list_disks | 列出所有磁盘设备 | - | |
| 4 | list_filesystems | 列出所有文件系统 | - | |
| 5 | list_users | 列出所有用户 | - | |
| 6 | disk_usage | 磁盘/内存空间 | - | |
| 7 | check_state | 实时状态(L3 预检) | area | |
| 8 | apply_config | 应用 OMV 配置变更 | - | |
| 9 | clean_caches | 清缓存(apt+tmp+日志) | - | |
| 10 | check_updates | 检查可升级包 | - | |
| 11 | network_info | 网络接口信息 | - | |
| 12 | reboot_system | 重启系统 | - | ⚠️ |

### 4.2 docker-agent(15 个)
容器管理:启停/日志/镜像/compose/资源/执行

| # | action | 说明 | 参数 | 破坏性 |
|---|---|---|---|---|
| 1 | list_containers | 列出所有容器 | - | |
| 2 | start_container | 启动容器 | name* | |
| 3 | stop_container | 停止容器 | name* | |
| 4 | restart_container | 重启容器 | name* | |
| 5 | container_logs | 查看日志 | name*, lines | |
| 6 | container_stats | 资源使用(CPU/内存) | name* | |
| 7 | container_exec | 容器内执行命令 | name*, command* | |
| 8 | pull_image | 拉取镜像 | image* | |
| 9 | list_images | 列出本地镜像 | - | |
| 10 | prune_images | 清理无用镜像 | - | ⚠️ |
| 11 | health_check | 容器健康检查 | - | |
| 12 | compose_up | 启动 compose 编排 | file | |
| 13 | compose_down | 停止 compose 编排 | file | |
| 14 | compose_logs | compose 项目日志 | file, lines | |
| 15 | generate_compose | 生成 compose 模板 | name*, image*, port, volume, network, env, output | |

### 4.3 filesystem-agent(19 个)
文件管理:共享/文件/搜索/分类/去重/标签/文件操作/共享创建删除

| # | action | 说明 | 参数 | 破坏性 |
|---|---|---|---|---|
| 1 | list_shared_folders | 列出共享文件夹 | - | |
| 2 | list_files | 列出目录文件 | path* | |
| 3 | file_info | 文件详情(stat) | path* | |
| 4 | search_files | 搜索文件(find) | path*, pattern* | |
| 5 | dir_size | 目录大小(du) | path* | |
| 6 | classify_files | 按扩展名分类 | path* | |
| 7 | dedup | md5 去重 | path* | |
| 8 | tag_file | 打标签 | path*, tag* | |
| 9 | search_by_tag | 按标签搜索 | tag* | |
| 10 | list_tags | 列出所有标签 | - | |
| 11 | delete_tag | 删除标签 | path*, tag* | |
| 12 | auto_tag_directory | 自动标签目录 | path* | |
| 13 | move_file | 移动文件 | src*, dest* | |
| 14 | copy_file | 复制文件 | src*, dest* | |
| 15 | create_directory | 创建目录 | path* | |
| 16 | file_permissions | 查看权限 | path* | |
| 17 | create_share | 创建共享(哨兵UUID+FsTab) | name* | |
| 18 | create_smb_share | 创建SMB共享(哨兵UUID) | name* | |
| 19 | delete_share | 删除共享 | name* | ⚠️ |

### 4.4 backup-agent(6 个)
备份管理:rsync 增量/恢复/校验/定时/状态

| # | action | 说明 | 参数 | 破坏性 |
|---|---|---|---|---|
| 1 | backup | 增量备份(rsync --delete) | src*, dest* | |
| 2 | restore | 从备份恢复 | src*, dest* | |
| 3 | verify | 校验一致性(dry-run) | src*, dest* | |
| 4 | list_backups | 列出备份内容 | path | |
| 5 | backup_status | 备份状态+磁盘空间 | dest | |
| 6 | schedule_backup | 定时备份(crontab) | src*, dest*, schedule | |

### 4.5 vision-agent(6 个)
相册管理:扫描/视觉打标/批量/搜索/相册/统计

| # | action | 说明 | 参数 | 破坏性 |
|---|---|---|---|---|
| 1 | scan_photos | 扫描照片文件 | path* | |
| 2 | tag_photo | 单张打标(vision 模型) | path* | |
| 3 | tag_all_photos | 批量打标+存标签库 | path* | |
| 4 | search_photos | 按标签搜索 | query* | |
| 5 | build_album | 智能相册(按主题) | theme* | |
| 6 | photo_stats | 标签统计(总数/排名) | - | |

### 4.6 media-agent(4 个)
影音管理:TMDB刮削/字幕/转码/媒体库

| # | action | 说明 | 参数 | 破坏性 |
|---|---|---|---|---|
| 1 | scrape_metadata | 刮削元数据(TMDB优先) | path* | |
| 2 | find_subtitle | 匹配字幕(需配置) | path* | |
| 3 | transcode | 格式转换(ffmpeg) | input*, output* | |
| 4 | media_library_scan | 扫描媒体库+解析元数据 | path* | |

### 4.7 download-agent(5 个)
下载管理:qBittorrent API/暂停恢复/移动入库

| # | action | 说明 | 参数 | 破坏性 |
|---|---|---|---|---|
| 1 | add_torrent | 添加磁力链接 | magnet* | |
| 2 | list_downloads | 列出下载任务 | - | |
| 3 | pause_torrent | 暂停下载 | hash, name | |
| 4 | resume_torrent | 恢复下载 | hash, name | |
| 5 | move_to_library | 移动到媒体库 | src*, dest* | |

### 4.8 homeassistant-agent(6 个)
智能家居:设备/状态/服务调用/自动化/事件

| # | action | 说明 | 参数 | 破坏性 |
|---|---|---|---|---|
| 1 | list_entities | 列出所有 HA 设备 | - | |
| 2 | get_state | 查指定设备状态 | entity_id* | |
| 3 | call_service | 调用服务控制设备 | domain*, service*, payload | |
| 4 | list_services | 列出可用服务 | - | |
| 5 | list_automations | 列出自动化规则 | - | |
| 6 | fire_event | 触发自定义事件 | event_type*, payload | |

### 4.9 cloud-agent(4 个)
网盘管理:经 Alist 适配百度/阿里云盘,列文件/上传/下载/信息

| # | action | 说明 | 参数 | 破坏性 |
|---|---|---|---|---|
| 1 | list_cloud | 列网盘文件 | path | |
| 2 | download_from_cloud | 从网盘下载到 NAS | remote_path*, local_path | |
| 3 | upload_to_cloud | 从 NAS 上传到网盘 | local_path*, remote_path | |
| 4 | cloud_info | 已配置云存储列表 | - | |

---

## 5. 调度模式知识库(kb-tasks.json,13 个)

| 模式 | 步数 | 步骤序列 |
|---|---|---|
| 创建SMB共享 | 3 | create_share → create_smb_share → apply_config |
| 备份任务 | 3 | check_state(disk) → backup → list_backups |
| 备份验证 | 3 | backup → verify → backup_status |
| 照片打标签 | 3 | scan_photos → tag_all_photos → photo_stats |
| 容器部署 | 4 | generate_compose → pull_image → compose_up → health_check |
| 容器故障排查 | 3 | list_containers → container_logs → health_check |
| 容器重建 | 4 | compose_down → pull_image → compose_up → health_check |
| 系统健康检查 | 4 | system_info → check_smart → disk_usage → health_check |
| 磁盘空间清理 | 4 | disk_usage → prune_images → clean_caches → disk_usage |
| 文件整理 | 4 | classify_files → dedup → auto_tag_directory → list_tags |
| 媒体库整理 | 1 | media_library_scan |
| HA设备控制 | 2 | list_entities → call_service |
| HA自动化查看 | 2 | list_automations → list_services |

---

## 6. Sentinel 审计

独立 systemd 进程,接收 Core 的所有事件并记录:
- `ai.request` - 用户请求
- `ai.scheduled` - 调度决策(agent + action)
- `ai.plan_scheduled` - 多步 Plan 启动
- `ai.agent_result` - agent 执行结果
- `ai.destructive_pending` - 破坏性操作待确认
- `ai.destructive_confirmed` - 破坏性操作已确认执行
- `ai.direct_reply` - 直接回复(无调度)
- `ai.applied` - 配置已应用

日志文件: `/var/log/aaos/sentinel.log`

---

## 7. OMV 集成

### PHP RPC 插件(openmediavault-aaos)
- RPC 服务名: `AAOS`
- 方法: `query`(代理到 Core socket) + `status`(进程状态)
- DEB 包: `openmediavault-aaos_0.1.0-1_all.deb`
- 调用: `omv-rpc "AAOS" "query" '{"input":"列出磁盘"}'`

### Angular 聊天面板
- 路由: `/services/aaos`
- 菜单: Services > AAOS
- 组件: `AaosChatPageComponent`(Angular 15)
- 功能: 自然语言输入 + 消息列表 + 破坏性确认按钮
- 访问: `http://<nas-ip>/` → 登录 admin → Services > AAOS

---

## 8. systemd 服务

| 服务 | 说明 | 自启 |
|---|---|---|
| `aaos-core.service` | Core Agent(LLM 调度) | ✅ enabled |
| `aaos-sentinel.service` | Sentinel(审计日志) | ✅ enabled |
| `homeassistant.service` | HA 容器(podman) | ✅ enabled |
| `aaos-daily-check.timer` | 每日巡检(08:00) | ✅ enabled |

Core 服务配置:
```ini
[Service]
EnvironmentFile=/etc/aaos/env    # ARK_API_KEY + HA_TOKEN
ExecStart=/usr/local/bin/aaos-core --socket /run/aaos/core.sock --sentinel-socket /run/aaos/sentinel.sock
```

---

## 9. 配置文件

| 文件 | 位置 | 说明 |
|---|---|---|
| `models.toml` | `/etc/aaos/` | 模型库(provider/model/fallback) |
| `agents.json` | `/etc/aaos/` | Agent 注册表(8 agent / 70 action) |
| `kb-context.json` | `/etc/aaos/` | L1 系统上下文(明确任务目标/对象) |
| `kb-tasks.json` | `/etc/aaos/` | 调度模式(13 个任务模板) |
| `kb-nas.json` | `/etc/aaos/` | OMV RPC 方法目录(265 方法) |
| `env` | `/etc/aaos/` | 环境变量(ARK_API_KEY, HA_TOKEN) |

---

## 10. CLI 使用

```bash
# 自然语言交互
aaos-cli 系统信息
aaos-cli 列出磁盘
aaos-cli 创建共享叫newshare
aaos-cli 删除共享叫testshare          # 会要求确认
aaos-cli --confirm <token>             # 确认破坏性操作
aaos-cli "备份/tmp到/tmp/bak"          # 多步 Plan

# 工具命令
aaos-cli --list-models                 # 列模型库
aaos-cli --discover-models coding-plan # 自动发现模型
aaos-cli --test-llm                    # 测 LLM 连通性
aaos-cli --daily-check                 # 手动触发巡检
```

---

## 11. OMV RPC API 约定

### 调用形式
```bash
omv-rpc "<Service>" "<method>" '<json params>'   # root 免登录
```

### 关键约定
- **异步方法**: 返回 `/tmp/bgstatus*` 路径,需轮询 `running=false`
- **新建对象**: uuid 传哨兵 `fa4b1c66-ef79-11e5-87a0-0002b3a176b4`
- **字段类型**: boolean/string/integer/enum,按 datamodels/*.json schema
- **写后部署**: `omv-salt deploy run <service>` 或 `Config::applyChangesBg`
- **mntentref**: 用 FsTab 条目 UUID(不是文件系统 UUID),查 `FsTab::getByFsName`

### 核心 RPC 服务(31 个 / 265 方法)
Apt / CertificateMgmt / Config / Cron / DiskMgmt / EnvVars / Exec / FileSystemMgmt / FolderBrowser / FsTab / Iptables / LogFile / Network / Notification / PerfStats / PluginMgmt / PowerMgmt / Quota / Rrd / Rsync / Rsyncd / Services / ShareMgmt / Smart / Smb / Ssh / System / UserMgmt / WebGui / Cron / Certificatemgmt

---

## 12. 源码结构

```
truenas-aaos/
├── Cargo.toml                    # Rust 项目(toml + reqwest + serde + tokio + clap)
├── models.toml                   # 模型库配置
├── agents.json                   # Agent 注册表(8 agent / 70 action)
├── kb-context.json               # L1 系统上下文
├── kb-tasks.json                 # 调度模式(13 模式)
├── kb-nas.json                   # OMV RPC 方法目录(265 方法)
├── src/
│   ├── lib.rs                    # 模块声明
│   ├── llm.rs                    # Core LLM 客户端 + 调度 + 翻译
│   ├── omv.rs                    # omv-rpc 调用层 + 知识库加载
│   ├── models.rs                 # 模型库(Provider/Model + 自动发现)
│   ├── intent.rs                 # 规则意图解析(fallback)
│   ├── ipc.rs                    # Unix Socket IPC 协议
│   ├── agents/
│   │   ├── mod.rs                # Intent/Plan/ActionResult + dispatch
│   │   ├── system.rs             # system-agent(9 action)
│   │   ├── docker.rs             # docker-agent(15 action)
│   │   ├── filesystem.rs         # filesystem-agent(19 action)
│   │   ├── backup.rs             # backup-agent(6 action)
│   │   ├── vision.rs             # vision-agent(6 action)
│   │   ├── media.rs              # media-agent(4 action)
│   │   ├── download.rs           # download-agent(5 action)
│   │   └── ha.rs                 # homeassistant-agent(6 action)
│   └── bin/
│       ├── core.rs               # Core Agent 主进程
│       ├── sentinel.rs           # Sentinel 审计进程
│       └── cli.rs                # aaos-cli 命令行
├── systemd/
│   ├── aaos-core.service
│   └── aaos-sentinel.service
└── omv-aaos/
    └── openmediavault-aaos/      # OMV PHP 插件
        ├── usr/share/openmediavault/engined/rpc/aaos.inc
        ├── usr/share/openmediavault/datamodels/rpc.aaos.json
        ├── usr/share/openmediavault/workbench/              # Angular 面板
        └── debian/                                           # DEB 打包
```

---

## 13. 开发历程

| 日期 | 里程碑 |
|---|---|
| 07-23 | 论文解读 + TrueNAS 选型 |
| 07-26 | TrueNAS 弃用(3 大死穴) → OMV 8 选定 |
| 07-26 | OMV 深度研究 + 命令目录(265 方法) + 端到端闭环 |
| 07-26 | OMV PHP 插件 + Angular 聊天面板 |
| 07-27 | 模型库 + 火山 coding plan + Core LLM 调度 |
| 07-29 | Agent + KB 架构(function-calling 查 KB) + 三层知识库 |
| 07-29 | 8 个 Execution Agent 全部实现 |
| 07-30 | 多步 Plan + 写操作闭环(哨兵 UUID + FsTab) |
| 07-30 | 破坏性确认 + systemd 正式化 + Web UI 验证 |
| 07-31 | Agent 能力扩展(55→70 action) + KB 驱动调度(kb-tasks.json) |

---

## 14. 待完成

- [ ] 浏览器实测 Web UI(人工)
- [ ] 真机部署(Debian ISO U 盘已就绪)
- [ ] HA 接真实设备(call_service 实测)
- [ ] vision-agent 真实照片打标实测
- [ ] media-agent TMDB API key 配置 + ffmpeg 安装
- [ ] download-agent qBittorrent 部署 + QBIT_HOST 配置
- [ ] 安全加固(HTTPS + Sentinel 拦截 + key 管理)
- [ ] Charter 权限系统(论文的 read/write/destructive 分级)

---

*文档生成时间: 2026-07-31*
*AAOS-NAS v0.1.0 | OMV 8.5.5-1 | deepseek-v4-pro*
