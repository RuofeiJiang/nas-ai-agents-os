# AAOS 微信桥

微信消息 -> AAOS core.sock -> 回复发回微信。让微信成为 AAOS 的聊天前端。

基于腾讯官方 `@tencent-weixin/openclaw-weixin` 协议(**不装 OpenClaw 框架**),扫码登录个人微信。AAOS 零改动(只读 core.sock)。

## 前置
- AAOS 已部署并运行(core.sock 可用)
- Node.js 22+
- 个人微信(扫码登录)

## 安装
```bash
cd wechat-bridge
npm install
cp .env.example .env   # 编辑配置
```

## 配置(.env)
- `AAOS_SOCKET`:core.sock 路径(默认 `/run/aaos/core.sock`)
- `ALLOWED_USERS`:微信用户 ID 白名单(逗号分隔;空 = 允许所有,仅测试用)

获取用户 ID:先留空启动,给自己发条消息,看日志 `[bot] 收到消息 from=xxx`,把 xxx 填入 `ALLOWED_USERS`。

## 运行
```bash
npm run dev
```
首次启动终端显示二维码,用手机微信扫码授权。凭证自动保存,重启免扫码。

## 测试
微信发消息(和 aaos-cli / Web UI 一样):
- `列出磁盘` -> AAOS 返回磁盘列表
- `系统信息` -> CPU/内存/uptime
- `列出容器` -> docker-agent

破坏性操作(如`删除共享`)会回复"需确认,请到 Web UI 操作"(微信暂不支持确认流程)。

## 安全
- **`ALLOWED_USERS` 务必配置**(否则任何微信联系人都能控制你 NAS)
- 破坏性操作走 Web UI 确认(微信拒绝)
- AAOS Sentinel 仍审计所有操作

## 换微信账号
```bash
npm run dev -- --logout
```

## 架构
```
微信消息 ──▶ getUpdates(iLink 长轮询)──▶ AaosChat.chat()
                                          │
                                          ▼ connect core.sock
                                   {"type":"request","input":"..."}
                                          │
微信回复 ◀── sendMessage ◀── output ◀── {"type":"response","output":"..."}
```

代码结构:
- `src/ai/aaos.ts` - AAOS socket client(核心,连 core.sock)
- `src/bot.ts` - 主循环(getUpdates -> AaosChat -> sendMessage)
- `src/weixin/` - 微信层(扫码登录 + iLink API,来自 wx-robot-ilink)
- `src/index.ts` - 入口
