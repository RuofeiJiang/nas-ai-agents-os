# NAS AI Agents OS (AAOS-NAS)

[![License](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-musl-orange.svg)](https://www.rust-lang.org/)
[![OMV](https://img.shields.io/badge/OMV-8-green.svg)](https://www.openmediavault.org/)

> 基于 AAOS 架构的 AI 原生 NAS 系统
> AI 作为原生系统服务管理 NAS 全部功能,用户用自然语言交互

## 什么是 AAOS-NAS

把 AAOS(AI Agents Operating System)架构植入 OpenMediaVault 8,实现:
- **Core Agent**:LLM(deepseek-v4-pro)理解自然语言,查知识库,调度执行
- **9 个 Execution Agent**:预设执行 + 域 LLM 智能分析(78 个 action)
- **三层知识库**:L1 通用(LLM 内置) + L2 系统(NAS 配置) + L3 实时状态
- **OMV 集成**:PHP RPC 插件 + Angular 聊天面板 + 自然语言操作全部 NAS 功能

## 架构

```
用户自然语言
  -> Core Agent(LLM):理解 + 查 KB + 调度
  -> Execution Agent(9个,预设):执行操作
  -> OMV(omv-rpc) / CLI(podman/rsync/ffmpeg) / 模型(vision)
  -> Core:结果翻译回自然语言
  -> 用户

Sentinel: 全程审计日志
每日巡检: systemd timer 08:00 自动健康检查
```

## 9 个 Execution Agent

| Agent | 功能 | Action 数 |
|---|---|---|
| system | 系统维护(SMART/磁盘/网络/缓存/更新) | 13 |
| docker | 容器管理(启停/compose/镜像/诊断) | 15 |
| filesystem | 文件管理(共享/搜索/分类/标签/创建) | 19 |
| backup | 备份(rsync/恢复/校验/定时) | 6 |
| vision | 相册(扫描/vision打标/相册/统计) | 6 |
| media | 影音(TMDB刮削/字幕/转码) | 4 |
| download | 下载(qBittorrent/暂停恢复) | 5 |
| homeassistant | 智能家居(设备/服务/自动化) | 6 |
| cloud | 云存储(百度/阿里网盘,经Alist) | 4 |

## 快速开始

### 1. 装 Debian 13 + OMV 8
```bash
wget -O - https://get.openmediavault.io | sh
```

### 2. 部署 AAOS
```bash
bash deploy-aaos.sh <NAS_IP>
```

### 3. 浏览器配置
```
http://<NAS_IP>/ -> Services > AAOS -> 设置 -> 填 API Key
```

详细步骤见 [DEPLOY-GUIDE.md](DEPLOY-GUIDE.md)

## 目录结构

```
nas-ai-agents-os/
├── src/                        # Rust 核心
│   ├── agents/                 # 9 个 Execution Agent + llm_helper
│   │   ├── mod.rs              # Intent/Plan/ActionResult + dispatch
│   │   ├── system.rs           # system-agent (13 action)
│   │   ├── docker.rs           # docker-agent (15 action)
│   │   ├── filesystem.rs       # filesystem-agent (19 action)
│   │   ├── backup.rs           # backup-agent (6 action)
│   │   ├── vision.rs           # vision-agent (6 action)
│   │   ├── media.rs            # media-agent (4 action)
│   │   ├── download.rs         # download-agent (5 action)
│   │   ├── ha.rs               # homeassistant-agent (6 action)
│   │   ├── cloud.rs            # cloud-agent (4 action)
│   │   └── llm_helper.rs       # Agent 域 LLM 智能分析
│   ├── bin/
│   │   ├── core.rs             # Core Agent (LLM 调度)
│   │   ├── sentinel.rs         # Sentinel (审计日志)
│   │   └── cli.rs              # CLI 工具
│   ├── llm.rs                  # LLM 客户端 + 调度 + 翻译
│   ├── omv.rs                  # omv-rpc 调用层
│   ├── models.rs               # 模型库 (provider/model/自动发现)
│   ├── intent.rs               # 规则意图解析 (fallback)
│   └── ipc.rs                  # Unix Socket IPC
├── omv-plugin/                 # OMV 集成
│   ├── openmediavault-aaos/    # PHP RPC 插件
│   └── workbench/              # Angular 15 聊天面板
├── systemd/                    # systemd 服务文件
├── Cargo.toml
├── models.toml                 # 模型库配置
├── agents.json                 # Agent 注册表 (9 agent / 78 action)
├── kb-context.json             # L2 系统知识库
├── kb-tasks.json               # 调度模式 (13 个任务模板)
├── kb-nas.json                 # OMV RPC 方法目录 (265 方法)
├── deploy-aaos.sh              # 一键部署脚本
├── DEPLOY-GUIDE.md             # 部署指南
└── AAOS-NAS-DOC.md             # 项目文档
```

## 核心特性

- **自然语言操作**: "列出磁盘" / "创建共享叫photos" / "备份到USB"
- **多步任务**: "创建SMB共享" = 3步Plan(创建共享→创建SMB→应用配置)
- **预检机制**: 操作前查磁盘空间/容器状态,不够直接告知用户
- **破坏性确认**: 删除/清理操作需用户确认
- **每日巡检**: systemd timer 每天08:00自动健康检查
- **快速回复**: 常见查询秒回(规则匹配,跳LLM)
- **LLM智能分析**: agent 域模型分析日志/诊断故障/制定策略
- **KB驱动调度**: 任务模式存知识库,不靠prompt
- **Key管理**: Web UI 改 API key,不经文件系统

## 技术栈

| 层 | 技术 |
|---|---|
| 底座 | OpenMediaVault 8 (Debian 13 trixie) |
| Core | Rust (musl 静态二进制) |
| LLM | deepseek-v4-pro (火山方舟 coding plan, OpenAI 兼容) |
| OMV | PHP RPC + Angular 15 |
| 容器 | podman |
| IPC | Unix Socket |

## 使用示例

```bash
# 快速查询(秒回)
aaos-cli 系统信息
aaos-cli 列出磁盘
aaos-cli 列出容器

# 多步任务
aaos-cli 创建共享叫photos
aaos-cli 备份/srv到/usb

# LLM 智能分析
aaos-cli 分析系统有没有异常
aaos-cli HA容器为什么没跑

# 破坏性操作(需确认)
aaos-cli 删除共享叫testshare
aaos-cli --confirm <token>

# 每日巡检
aaos-cli --daily-check
```

## License

AGPL-3.0

## 相关

- [AAOS 论文](https://ruofeijiang.github.io/2026/06/28/aaos-overview/) - AI Agents Operating System 理论
- [AAOS 自发调度问题](https://ruofeijiang.github.io/2026/07/09/spontaneity-problem/) - 自发调度 vs SOP 编排
- [AAOS 实现篇](https://ruofeijiang.github.io/2026/07/19/ai-agents-os-implementation/) - 从理论到工程实现
- [Sentinel 论文](https://github.com/RuofeiJiang/sentinel) - 独立安全守护系统
- [OpenMediaVault](https://www.openmediavault.org/) - NAS 底座
