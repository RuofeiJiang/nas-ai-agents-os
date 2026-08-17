# NAS AI Agents OS (AAOS-NAS)

[![License](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-musl-orange.svg)](https://www.rust-lang.org/)
[![Docker](https://img.shields.io/badge/Docker-WIP-2496ED.svg)](https://www.docker.com/)

> 一个 AI 大脑，让你的 NAS 听懂自然语言。

> ⚠️ **项目状态**：开发中 (v0.1.0)。OMV 8 已在**虚拟机 + 裸机真机**完成端到端部署验证（Pentium Gold 8505 / 16G / Debian trixie，见 [DEPLOY-GUIDE.md](DEPLOY-GUIDE.md)）；Docker 打包进行中（未验证）；群晖 / QNAP / TrueNAS / Unraid 等多平台适配规划中。

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

## 核心特性

- **自然语言交互**：说人话，不用记命令
- **多步任务编排**："创建 SMB 共享" 自动拆成 3 步执行
- **预检机制**：操作前自动检查磁盘空间/容器状态
- **破坏性确认**：删除/清理操作需 token + 安全口令(AAOS_SAFE_PWD)二次确认,三前端(CLI/Web/微信)统一
- **每日巡检**：每天 08:00 自动健康检查
- **规则优先**：常见查询秒回，跳过 LLM
- **三层知识库**：L1 通用 + L2 系统 + L3 实时状态，准确率 100%（17/17）
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

- [AAOS 论文](https://ruofeijiang.github.io/2026/06/28/aaos-overview/) - AI Agents Operating System 理论
- [AAOS 自发调度问题](https://ruofeijiang.github.io/2026/07/09/spontaneity-problem/)
- [AAOS 实现篇](https://ruofeijiang.github.io/2026/07/19/ai-agents-os-implementation/)
- [Sentinel 论文](https://github.com/RuofeiJiang/sentinel) - 独立安全守护系统

## License

AGPL-3.0