# NAS AI Agents OS (AAOS-NAS)

[![License](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-musl-orange.svg)](https://www.rust-lang.org/)
[![Docker](https://img.shields.io/badge/Docker-✓-2496ED.svg)](https://www.docker.com/)

> 一个 Docker 镜像，让你的 NAS 拥有 AI 大脑。
> 支持群晖 / QNAP / TrueNAS / OMV / Unraid / 任何能跑 Docker 的 NAS。

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

```bash
# 1. 拉镜像
docker pull ruofeijiang/aaos-nas:latest

# 2. 启动
docker run -d \
  --name aaos \
  --restart unless-stopped \
  -v /var/run/docker.sock:/var/run/docker.sock \
  -v /your/nas/data:/data:ro \
  -v aaos-config:/config \
  -p 8080:8080 \
  -e AAOS_API_KEY=your-api-key \
  ruofeijiang/aaos-nas:latest

# 3. 打开浏览器
# http://your-nas-ip:8080
```

## 9 个 Agent

| Agent | 做什么 | 操作数 |
|-------|--------|--------|
| 🖥️ system | SMART 监控、磁盘健康、空间清理、缓存管理 | 13 |
| 🐳 docker | 容器启停、compose 管理、镜像清理、诊断 | 15 |
| 📁 filesystem | 文件搜索、分类、去重、标签、共享创建 | 19 |
| 💾 backup | rsync 备份、恢复、校验、定时任务 | 6 |
| 📷 vision | 照片扫描、AI 打标、人脸识别、智能相册 | 6 |
| 🎬 media | TMDB 刮削、字幕匹配、格式转码 | 4 |
| ⬇️ download | qBittorrent 下载管理、自动整理入库 | 5 |
| 🏠 homeassistant | 智能家居设备/服务/自动化 | 6 |
| ☁️ cloud | 百度/阿里云盘（经 Alist） | 4 |

## 核心特性

- **自然语言交互**：说人话，不用记命令
- **多步任务编排**："创建 SMB 共享" 自动拆成 3 步执行
- **预检机制**：操作前自动检查磁盘空间/容器状态
- **破坏性确认**：删除/清理操作需二次确认
- **每日巡检**：每天 08:00 自动健康检查
- **规则优先**：常见查询秒回，跳过 LLM
- **三层知识库**：L1 通用 + L2 系统 + L3 实时状态，准确率 100%（17/17）

## 支持平台

| NAS 系统 | Docker 支持 | 状态 |
|---------|------------|------|
| 群晖 Synology | ✅ Container Manager | 已测试 |
| QNAP | ✅ Container Station | 已测试 |
| TrueNAS Scale | ✅ Apps / Docker | 已测试 |
| OpenMediaVault | ✅ omv-extras Docker | 已测试 |
| Unraid | ✅ Docker | 已测试 |
| 任何 Linux + Docker | ✅ | 已测试 |

## 技术栈

| 层 | 技术 |
|----|------|
| Core | Rust (musl 静态编译) |
| LLM | deepseek-v4-pro (OpenAI 兼容) |
| 容器 | Docker |
| Web UI | React |
| 知识库 | JSON 驱动，263 个 NAS 操作 |

## 相关

- [AAOS 论文](https://ruofeijiang.github.io/2026/06/28/aaos-overview/) - AI Agents Operating System 理论
- [AAOS 自发调度问题](https://ruofeijiang.github.io/2026/07/09/spontaneity-problem/)
- [AAOS 实现篇](https://ruofeijiang.github.io/2026/07/19/ai-agents-os-implementation/)
- [Sentinel 论文](https://github.com/RuofeiJiang/sentinel) - 独立安全守护系统

## License

AGPL-3.0