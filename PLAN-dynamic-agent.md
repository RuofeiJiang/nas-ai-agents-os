# 动态 Agent 自扩展方案:部署即纳管

## 目标
docker-agent 部署一个容器服务后,自动发现其 API、生成管理它的 KB + 注册 agent,无需手写 Rust。把"调 REST API 的 agent"(cloud/ha/download + 潜在 game/...)抽象成一个数据驱动的通用执行器。RomM 是第一个验证样本。

## 核心思路
从"编译期 agent(每服务一份 Rust)"升级为"运行期 agent(一份 generic-http-agent + N 份 kb-\<service\>.json)"。契合 KB 驱动原则:架构(通用执行器)提供 agency,KB 描述知识,LLM 兜底。

## 架构改动(5 块)

### 1. generic-http-agent(新 `src/agents/generic.rs`)
通用 HTTP 执行器,不硬编码任何服务。抽象自 cloud.rs 的壳。
- `execute(agent_name, action, args)`:读 `/etc/aaos/kb-<agent_name>.json`,按 action 的 method/path/params 组装 HTTP 请求,注入鉴权,调服务,按 `response_path` 提取响应,返回 ActionResult
- `smart_execute`:KB 未覆盖的 action -> 域 LLM 分析(复用 `llm_helper::smart_analyze`,和现有 9 agent 一致)
- 鉴权:按 KB 的 `auth` 配置(bearer/api_key/basic/none)从 env 读 token 注入 header
- path param(`/api/roms/{id}`)自动替换;query param 自动拼到 URL

### 2. dispatch 改造(`src/agents/mod.rs`)
现硬编码 match 9 个 agent -> 加动态 fallback:
```rust
match intent.agent.as_str() {
    "system" => system::SystemAgent.execute(...),  // 9 个已知 agent 硬编码(快)
    other => generic::GenericHttpAgent.execute(other, &intent.action, &intent.args).await,
}
```
未知 agent 自动走 generic-http-agent(从 agents.json 查它的 kb 文件)。

### 3. 服务发现 + OpenAPI->KB(新 `src/agents/discover.rs`)
- `discover_service(name, base_url)`:依次试 `/openapi.json` `/swagger.json` `/v3/api-docs` `/api/openapi.json` -> 命中调 `openapi_to_kb`
- `openapi_to_kb(spec, name)`:遍历 paths×methods -> actions
  - `summary`/`description` -> action description(语义)
  - `parameters` -> params(name/in/type/required)
  - method 推 class:GET=read, POST/PUT/PATCH=write, DELETE=destructive
  - 过滤内部端点(`/auth/login` `/health` `/heartbeat` 等)

### 4. 动态注册(agents.json 运行时更新)
discover 成功后:
- 写 `/etc/aaos/kb-<name>.json`
- 往 agents.json 加一条 `{name, description, type:"http", kb:"kb-<name>.json", actions:[...]}`
- Core 的 `list_agents` 工具自动看到新 agent -> NL 可调度

### 5. 部署钩子(`src/agents/docker.rs`)
新 action `deploy_service`:generate_compose -> compose_up -> 等容器健康 -> discover_service -> 生成 KB -> 注册。把"部署 + 自动纳管"串成一个动作(或 Core 的多步 Plan)。

## KB schema(kb-\<service\>.json)
```json
{
  "domain": "romm",
  "source": "rommapp/romm:latest",
  "base_url_env": "ROMM_URL",
  "auth": {"type": "bearer", "token_env": "ROMM_TOKEN", "header": "Authorization", "prefix": "Bearer"},
  "actions": {
    "list_platforms": {"method":"GET","path":"/api/platforms","description":"列出平台","class":"read","params":[],"response_path":""},
    "list_games": {"method":"GET","path":"/api/roms","description":"列出游戏","class":"read","params":[{"name":"platform_id","in":"query","type":"integer","required":false}],"response_path":"data"},
    "scan_library": {"method":"POST","path":"/api/platforms/{platform_id}/scan","description":"扫描库+刮削","class":"write","params":[{"name":"platform_id","in":"path","type":"integer","required":true}]}
  }
}
```

## 三档发现策略(质量递减、覆盖递增)
1. **模板库**(`/etc/aaos/templates/`):已知服务(RomM/Navidrome/Jellyfin/qBittorrent)精修 KB,镜像名匹配实例化 —— 质量最高,带语义
2. **OpenAPI 自动**:FastAPI/标准服务 —— 覆盖广,缺语义
3. **LLM 读文档**:无 spec —— 兜底,仅容器层管理

## 安全(必须)
- 自动 KB 按 HTTP method 标 class;DELETE/写操作走现有 Sentinel 破坏性确认(`is_destructive_action` + `PendingIntent`)
- 自动注册 agent:read 直接执行,write/destructive 需确认
- 鉴权 token 经 Key 管理(getConfig/setConfig),用户在 Web UI 配,不直接碰 env

## 落地阶段
- **P1 generic-http-agent + KB schema**:`generic.rs` + dispatch 动态分支 + 单测(mock KB 测 GET/POST/path/query/auth 注入)
- **P2 OpenAPI->KB 转换**:`discover.rs` + `openapi_to_kb` + 单测(OpenAPI 样本)
- **P3 部署钩子 + 动态注册**:`docker.rs deploy_service` + agents.json 运行时更新 + 鉴权接 Key 管理
- **P4 验证(RomM 样本)**:部署 RomM -> 自动发现 `/openapi.json` -> `kb-romm.json` -> 注册 -> NL"扫描游戏库"跑通;把 cloud-agent 迁到 generic 验证"一份代码 N 份 KB"
- **P5(可选)** 模板库 + LLM 兜底

## 风险
- OpenAPI 缺语义:description 空的端点 LLM 调度难(模板/LLM 补)
- 鉴权多样:自动发现 auth 类型难,需用户配 token
- RomM `/openapi.json` 实际质量未验证(P4 验证,不足走模板)
- 写操作安全:必须接 Sentinel,不能裸跑自动生成的写端点

## 不破坏现有架构
- 9 个硬编码 agent 不动(快/确定)
- generic-http-agent 只处理"未知 agent"(增量)
- KB 驱动 + smart_execute 兜底的模式和现有 agent 一致
- 动态 agent 的 Intent/ActionResult/Plan/Sentinel 确认全复用现有协议
