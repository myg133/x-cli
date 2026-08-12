# 范例 5：auth.yaml 自动登录 + 401 retry（v0.1 推荐模式）

> **Scope**：详见 `scope.md` L2 + `auth-patterns.md` Mode 5。
>
> **对比 Mode 2/3（手工 curl 拿 token）**：本模式 agent 零鉴权代码 —— agent 只发 JSON-RPC,x-cli 负责登录 / token 缓存 / 401 retry。
>
> **适用**：后端用 JWT / Bearer / 自定义 login 端点。**dev/demo 限定明文 creds**；生产环境推荐 v0.2+ 的 keychain / env var 方案。

## 端到端流程（5 步）

```
OpenAPI 源 ──1──> emit skill ──2──> 配 auth.yaml ──3──> 起 serve ──4──> 验证 ──5──> 用
                                   ↑ (x emit 自动产出 auth.example.yaml 模板)
```

## 步骤

### 1. 拿 OpenAPI

任意后端都行。本范例用 petstore + 一个**虚构的 JWT login 端点**演示完整流程。

### 2. emit skill（会自动产出 auth.example.yaml）

```bash
x emit examples/petstore.yaml --out ./generated/petstore-skill
```

**预期输出**：

```
✓ 解析 5 个接口、0 个工作流,格式 markdown 写入 ./generated/petstore-skill
```

**自动产出 3 个新文件**（v0.1 新行为）：

```
./generated/petstore-skill/
├── SKILL.md                # 总索引(顶部加 auth.yaml 提示段)
├── endpoints/*.md          # 每个 endpoint 详细
├── .x-cli/ir.json          # serve 用的 IR
├── auth.example.yaml       # ✅ 新增 —— auth.yaml 模板(in git)
└── .gitignore              # ✅ 新增 —— auth.yaml 等不进 git
```

### 3. 配 auth.yaml（从模板 cp + 填 creds）

```bash
cp ./generated/petstore-skill/auth.example.yaml ./generated/petstore-skill/auth.yaml
```

编辑 `auth.yaml` —— 填 login URL / creds / response token 字段：

```yaml
version: 1
token:
  # 启动时自动登录
  kind: login
  request:
    url: "https://api.example.com/api/v1/security/login"
    method: "POST"
    headers:
      Content-Type: "application/json"
    body:
      username: "admin"
      password: "your-password-here"
  response:
    token_path: "access_token"   # 默认 access_token;后端字段名不同就改
  refresh:
    on_401: true                  # 401 自动 re-login + retry,默认 true
```

**坑提醒**：serde flatten 让 LoginConfig 字段内联到 `token:` 下,**不要**多写一层 `login:`。

### 4. 启动 serve（自动登录）

```bash
x serve --skill ./generated/petstore-skill
```

**预期输出**：

```
✓ 从 ./generated/petstore-skill/auth.yaml 加载 session 配置
✓ 注入 1 个认证 header(来自 session)
# 等待 stdin 输入...
```

**背后做了什么**：

1. serve 读 `auth.yaml`,解析 AuthConfig
2. `Session::from_config` 自动 POST 到 `request.url`
3. 从响应里按 `token_path` 抽 token,注入 `Authorization: Bearer xxx`
4. 等 JSON-RPC 请求

### 5. 验证 + 业务调用

```bash
# ping —— 验证连通 + 鉴权 OK
echo '{"jsonrpc":"2.0","id":1,"method":"ping"}' | x serve --skill ./generated/petstore-skill

# 调真实业务 —— agent 不需要知道 token
echo '{"jsonrpc":"2.0","id":2,"method":"call","params":{"endpoint_id":"pet__get__pets_petId","path_params":{"petId":1}}}' | x serve --skill ./generated/petstore-skill
```

预期 ping 返回 `{"pong":true}`,业务请求带 `Authorization: Bearer ...` 自动注入。

## 401 retry 演示

**前置**：把后端的 token 强制设成过期（或在 mock server 里改逻辑：业务端点对过期 token 返回 401）。

**流程**：

1. serve 启动时 login 拿 token A
2. agent 调业务接口 → 后端看到 token A 过期 → 返回 401
3. x-cli 检测到 401 → 调 `Session::handle_401()` → 重新 login 拿 token B
4. x-cli 用 token B 重试原请求 1 次
5. 如果 token B 还是不行 → 401 透传给 agent,agent 报错给用户

**agent 完全感知不到 401 retry 发生过**。

## 多模式组合

auth.yaml 支持 bearer + login 同时存在(常用:主用 login + 备用 API key header):

```yaml
version: 1
token:
  kind: login
  request:
    url: "https://api.example.com/auth/login"
    body:
      username: "admin"
      password: "xxx"
  response:
    token_path: "access_token"
  refresh:
    on_401: true
```

如果还要塞额外的 header(如 `X-Tenant`),**当前 schema 不支持直接在 auth.yaml 里配**。两种方案:
- (1) 业务 endpoint 本身在 OpenAPI 里定义 `X-Tenant` 为 header param,agent 调 `call` 时传 `headers.X-Tenant`
- (2) 等 v0.2 加 `extra_headers` 字段

## 复用本模式到其他后端

| 后端 | login URL | body | response token 字段 |
|---|---|---|---|
| **Superset** | `https://x/api/v1/security/login` | `{"username","password"}` | `access_token` |
| **GitLab** | `https://gitlab.com/oauth/token` | `grant_type=password&username=x&password=y` | `access_token` |
| **自建 JWT 网关** | 通常 `POST /auth/login` | 视实现 | `token` / `access_token` / `data.access_token` |

把对应字段填到 `auth.yaml` 就完事 —— agent 代码 0 改动。

## 失败处理

| 现象 | 原因 + 修复 |
|---|---|
| serve 启动报 `parse auth.yaml: missing field` | YAML 缺字段(如 `request:` 没填) |
| serve 启动报 `login 端点返回 401` | creds 错 —— 改 `auth.yaml` 的 body |
| serve 启动报 `login 响应找不到 access_token` | 后端字段名不同 —— 改 `response.token_path`(dotted path 写嵌套) |
| 业务请求一直 401 | base-url 错 / token 真过期 re-login 也拿不到 —— 看 `troubleshooting.md` |
| `auth.yaml` 不进 git 丢失 | 是的,**有意设计** —— `auth.example.yaml` 是模板 |

## v0.2+ 计划

- `auth.refresh` JSON-RPC method(agent 主动强制刷新)
- 主动 refresh 按 `expires_in` 提前续
- Keychain 集成(Windows Credential Manager / macOS Keychain)
- OAuth2 client credentials(不需用户介入)