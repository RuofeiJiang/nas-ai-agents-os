# NAS AI Agents OS (AAOS-NAS)

[![License](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-musl-orange.svg)](https://www.rust-lang.org/)
[![Docker](https://img.shields.io/badge/Docker-WIP-2496ED.svg)](https://www.docker.com/)

> 一个 AI 大脑，让你的 NAS 听懂自然语言。

> ⚠️ **项目状态**：开发中 (v0.1.0)。OMV 8 已在**虚拟机 + 裸机真机**完成端到端部署验证（Pentium Gold 8505 / 16G / Debian trixie，见 [DEPLOY-GUIDE.md](DEPLOY-GUIDE.md)）；真机已完成**存储落地（3×HDD ext4 + 共享）+ 后端服务容器化（qBittorrent / Alist / Home Assistant）**；Docker 打包进行中（未验证）；群晖 / QNAP / TrueNAS / Unraid 等多平台适配规划中。

## 真机落地记录（2026-08-17）

裸机（Pentium Gold 8505 / 16G / 238G NVMe 系统盘）存储与服务现状：

| 层 | 内容 |
|---|---|
| 存储 | 3 块群晖拆机盘（4T/10T/6T）转 ext4 独立盘，不做 RAID |
| 共享 | `media`(10T) / `downloads`(6T) / `backup`(4T) |
| 容器 | qBittorrent(8080) / Alist(5244) / Home Assistant(8123, host+privileged) / Jellyfin(8096)，配置集中于 downloads 盘 `appdata/`，`podman-restart.service` 开机自启 |
| 影视链路 | `media/{movies,tv,anime}` 媒体树 + Jellyfin 展示端；下载完成的资源由 [media-organizer](scripts/media-organizer.py) 自动软链归位（跨盘不破坏做种） |
| SMB | media/downloads/backup 三共享已导出+授权+passdb，`bing` 账号可读写访问 |
| KB | 以上事实（挂载点/镜像/端口/凭据 env/镜像源约定）已写入 `kb-context.json`（L2），Core 调度可直接感知 |

**踩坑记录**（对二次部署直接有用）：

1. **OMV RPC 异步任务不能并发跑同一块盘**：`DiskMgmt.wipe` / `FileSystemMgmt.create` 都是后台任务（返回 bgstatus 路径），两批并发写同一块盘会把 GPT 写坏（`partx: failed to read partition table`）。必须串行 + 轮询 bgstatus 完成后再下一块。
2. **OMV 8 新建对象的魔法 UUID 因机而异**：`ShareMgmt.set` 新建时 uuid 要填本机 `/etc/default/openmediavault` 里的 `OMV_CONFIGOBJECT_NEW_UUID`（本机为 `fa4b1c66-ef79-11e5-87a0-0002b3a176b4`），不是网上流传的 `fa4bbae1-...`，填错报 XPath 查询失败。
3. **OMV 8 RPC 参数按 schema 全量校验**：如 `DiskMgmt.wipe` 必须带 `secure` 字段，缺了直接抛 SchemaValidationException。参数签名以 `/usr/share/openmediavault/datamodels/rpc.*.json` 为准。
4. **国内网络拉镜像必须走镜像源**：Docker Hub（connection reset）与 ghcr.io（403）直连均不可用，统一用 `docker.m.daocloud.io` 前缀（ghcr 仓库在 Hub 侧多有官方同步，如 `homeassistant/home-assistant`）。
5. **`models.toml` 只认 `api_key_env`，不认内联 `api_key`**：LLM 客户端（`src/llm.rs:23`）只从环境变量取 key，toml 里写 `api_key="..."` 会被静默忽略、请求无 Authorization -> 401。且 `core.rs` 的 `if let Ok(Some(raw))` 会吞掉 Err 不打日志，表现为「无法理解意图」而非鉴权错误——排障时先看 journal。


## 一句话

装上这个 Docker 容器，用自然语言管理你的 NAS：

> "清理重复照片" → AI 自动扫描、去重，释放 1.2GB  
> "帮我找去年在杭州拍的猫" → AI 人脸+场景+时间检索  
> "下载那个新出的电影，顺便配中文字幕" → 自动下载+刮削+字幕  
> "硬盘 3 号 SMART 有告警，什么情况？" → AI 诊断并给出建议

## 架构

```
用户（Web / App / 企微）
        │
┌───────▼──────────────────────────────┐
│           AAOS Core (Docker)          │
│                                       │
│  Core Agent (LLM)                     │
│  NL → 查知识库 → 调度 → 翻译结果      │
│                                       │
│  ┌──────┐ ┌──────┐ ┌──────┐ ┌──────┐ │
│  │system│ │docker│ │ file │ │backup│ │
│  │Agent │ │Agent │ │Agent │ │Agent │ │
│  └──────┘ └──────┘ └──────┘ └──────┘ │
│  ┌──────┐ ┌──────┐ ┌──────┐ ┌──────┐ │
│  │vision│ │media │ │downld│ │cloud │ │
│  │Agent │ │Agent │ │Agent │ │Agent │ │
│  └──────┘ └──────┘ └──────┘ └──────┘ │
│                                       │
│  Sentinel: 审计 + 权限守护            │
└───────────┬──────────────────────────┘
            │
    ┌───────┼───────┬─────────┐
    ▼       ▼       ▼         ▼
  群晖    QNAP   TrueNAS     OMV
 (任何能跑 Docker 的 NAS)
```

## 快速开始

> 当前推荐源码 + systemd 部署（已在 OMV 8 虚拟机 + 裸机真机端到端验证）。完整步骤见 [DEPLOY-GUIDE.md](DEPLOY-GUIDE.md)。

```bash
# 1. 构建（需 Rust + musl-tools）
cargo build --release --target x86_64-unknown-linux-musl

# 2. 部署到 NAS（OMV 8 / Debian）
sudo ./deploy-aaos.sh

# 3. 配置 LLM（火山方舟 coding plan 等）
sudo vim /etc/aaos/models.toml

# 4. 测试
aaos-cli "列出磁盘"
```

Docker 部署（`docker-compose.yml` 已就绪，**尚未验证**）：

```bash
cp docker.env.example .env   # 填入 API key
docker compose up -d
```

## 9 个内置 Agent + 动态扩展

| Agent | 做什么 | 操作数 |
|-------|--------|--------|
| 🖥️ system | SMART 监控、磁盘健康、空间清理、缓存管理 | 12 |
| 🐳 docker | 容器启停、compose 管理、镜像清理、诊断 | 15 |
| 📁 filesystem | 文件搜索、分类、去重、标签、共享创建 | 19 |
| 💾 backup | rsync 备份、恢复、校验、定时任务 | 6 |
| 📷 vision | 照片扫描、AI 打标、人脸识别、智能相册 | 6 |
| 🎬 media | TMDB 刮削、字幕匹配、格式转码 | 4 |
| ⬇️ download | qBittorrent 下载管理、自动整理入库 | 5 |
| 🏠 homeassistant | 智能家居设备/服务/自动化 | 6 |
| ☁️ cloud | 百度/阿里云盘（经 Alist） | 4 |

> 另有 **generic-http-agent**(数据驱动)+ 动态发现机制,见下方[动态 Agent 自扩展](#动态-agent-自扩展)。

## 动态 Agent 自扩展

部署 Docker 服务时自动生成管理它的 KB + agent,无需写 Rust。`generic-http-agent`(数据驱动)执行,三档发现策略 + 鉴权支持覆盖各类服务:

**三档发现**(从易到难,层层兜底)

| 档 | 机制 | 覆盖 |
|---|---|---|
| 1 自动 | OpenAPI 探测(`/openapi.json`) | FastAPI / 有 spec 的服务 |
| 2 自动回退 | DRF OPTIONS(Django/DRF 根 + OPTIONS) | 标准 Django/DRF 项目 |
| 3 手动 | LLM 读文档(README / 路由代码) | 无 spec + 非标准服务 |

**鉴权支持**:static token(bearer/api_key)+ **JWT 登录流程**(发凭证 -> 拿 token -> 调 API),覆盖需登录的服务。

**验证**:demo(FastAPI)openapi -> 4 action -> NL 调用;MusicTag(无 openapi + 自定义 DRF + JWT)LLM 读 `urls.py` -> 13 action。详见 [PLAN-dynamic-agent.md](PLAN-dynamic-agent.md)。

## 微信桥

AAOS 支持微信入口:微信消息 -> AAOS core.sock -> 回复发回微信,用自然语言在微信里控制 NAS。

- 基于腾讯官方 `@tencent-weixin/openclaw-weixin` 协议(**不装 OpenClaw 框架**),扫码登录个人微信
- AAOS 零改动(只读 core.sock,同 OMV 插件模式)
- 白名单鉴权 + 破坏性操作回复"确认 <安全口令>"执行(经 core.sock)

见 [wechat-bridge/](wechat-bridge/)。

## 非LLM自发性:两个实证(pt-watchdog / media-organizer)

自发性调度源自架构,而非 LLM--这是本项目的核心论点。目前有两个**零 LLM** 的自发行为组件作实证,恰好覆盖两种触发范式:

### 实证一:pt-watchdog(时钟驱动)

[scripts/pt-watchdog.py](scripts/pt-watchdog.py),systemd timer 每分钟扫描,无人触发地运行完整 agent 循环:

```
systemd timer(每分钟)          ← 架构赋予的"时钟",自发性的来源
  ├─ 感知:任务创建 3 分钟无速度?
  ├─ 决策:规则诊断(端口封禁/账号权限/死种/NAT死锁,查 tracker msg 分类)
  ├─ 行动:每 5 分钟自动 reannounce 自愈(qbit 周期性掉线的标准解法)
  └─ 报告:救不活 -> 告警写入收件箱 -> Web 聊天页展示(微信桥复用同一通道)
```

### 实证二:media-organizer(事件驱动)

[scripts/media-organizer.py](scripts/media-organizer.py),挂在 qBittorrent 的"下载完成"钩子上,下载完成那一刻自动归位:

```
qbit 完成钩子(架构事件)        ← 事件源,无需轮询
  ├─ 感知:新完成的种子(名称/路径/分类)
  ├─ 决策:纯规则分类--显式分类 > [发布组]→动漫 > SxxExx→剧集 > 中文→电影 > 兜底求助人工
  ├─ 行动:软链入媒体树(跨盘不破坏做种)
  └─ 报告:收件箱播报 + 触发 Jellyfin 库扫描
```

### 论点

两个组件的共同点:agency 由架构交付(触发源 + 规则化知识 + 预置动作 + 统一通知出口),LLM 不参与其中。LLM 在 AAOS 中的角色是语言理解与复杂推理(自然语言调度、结果翻译、读文档动态扩展 agent),两者互补而非等同--**去掉 LLM,自发行为仍在;去掉架构,LLM 只是一个被动的问答机**。

> 论文视角:这直接回应 1995 年 MAS 学派未竟的问题--agent 的自主性当时只能存在于理论形式化(BDI)中;AAOS 用架构机制把它工程化交付(时钟驱动与事件驱动两种范式),LLM 则补上了当年缺失的通用智能内核。详见[理论渊源](#相关)。

## 核心特性

- **自然语言交互**：说人话，不用记命令
- **多步任务编排**："创建 SMB 共享" 自动拆成 3 步执行
- **预检机制**：操作前自动检查磁盘空间/容器状态
- **破坏性确认**：删除/清理操作需 token + 安全口令(AAOS_SAFE_PWD)二次确认,三前端(CLI/Web/微信)统一
- **每日巡检**：每天 08:00 自动健康检查
- **规则优先**：常见查询秒回，跳过 LLM
- **三层知识库**：L1 通用 + L2 系统 + L3 实时状态，准确率 100%（17/17）
  - ⚠️ **KB 分层须知**：预设 agent 的 KB（如 [kb-download.json](kb-download.json)）目前混含两层内容--**通用知识**（API 端点/鉴权流程/故障模式库，如 6881 被 PT 站封禁、reannounce 解法）应随发行版共享；**机器实况**（监听端口/落盘路径/账号状态）属于单机，规范存放地是 `kb-context.json`（部署时按真机配置，见 DEPLOY-GUIDE 第六步），不应从模板继承。后续计划把两者拆分，机器层由部署时的 discover 动作现场生成（与动态 agent 的自动发现同机制）
- **🔄 动态自扩展**：部署服务自动生成 KB + agent(三档发现:OpenAPI / DRF OPTIONS / LLM 读文档),支持 JWT 登录鉴权,无需写代码
- **多端接入**：Web UI / CLI / 微信(扫码登录个人微信,经 core.sock)

## 支持平台

| NAS 系统 | 适配方式 | 状态 |
|---------|---------|------|
| OpenMediaVault 8 | 源码 + systemd / OMV 插件 | ✅ 已验证（VM + 裸机真机） |
| 任何 Linux + Docker | Docker compose | 🔧 进行中（未验证） |
| 群晖 / QNAP / TrueNAS / Unraid | Docker | 📋 规划中 |

## 技术栈

| 层 | 技术 |
|----|------|
| Core | Rust (musl 静态编译) |
| LLM | deepseek-v4-pro (OpenAI 兼容) |
| 容器 | Docker |
| Web UI | Angular（OMV workbench 插件） |
| 知识库 | JSON 驱动，263 个 NAS 操作 |

## 相关

### 理论渊源（AAMAS 学派经典）

本项目的核心问题--agent 的自发能力如何由架构交付--延续 MAS（多智能体系统）学派的问题意识。这一学派在 1995 年已建立完整的 agent 理论，受限于当时缺失的通用推理能力而未能走向日常用户；LLM 补上了这块短板，使"人人可用的 agent 操作系统"从理论构想变为工程现实：

- Wooldridge M. & Jennings N. R. (1995). *Intelligent Agents: Theory and Practice*. Knowledge Engineering Review. -- agent 概念的经典界定（智能性/自主性/社会性/反应性/主动性）
- Rao A. S. & Georgeff M. P. (1995). *BDI Agents: From Theory to Practice*. ICMAS-95. -- 信念-愿望-意图架构，agent 内部状态的经典形式化

### 作者文章

- [AAOS 论文](https://ruofeijiang.github.io/2026/06/28/aaos-overview/) - AI Agents Operating System 理论
- [AAOS 自发调度问题](https://ruofeijiang.github.io/2026/07/09/spontaneity-problem/)
- [AAOS 实现篇](https://ruofeijiang.github.io/2026/07/19/ai-agents-os-implementation/)
- [Sentinel 论文](https://github.com/RuofeiJiang/sentinel) - 独立安全守护系统

## License

AGPL-3.0