# 鉴权模式（5 种 + 选型表）

> **scope**：详见 `scope.md`（3 层模型 + session 生命周期）。本文只列具体写法,**所有写法都在 scope 内**。
>
> **新增**：v0.1+ 推荐用 **Mode 5（auth.yaml）** 自动管理 token 生命周期 —— agent 不写鉴权代码,x-cli serve 自动登录 + 401 retry。

## 5 种模式

### 1. 无鉴权（内部 API / mock server）

```bash
x serve --skill ./generated/petstore-skill
```

**适用**：公司内部 API（K8s 内网 service）/ mock server / 公共匿名 API（httpbin、petstore demo）

**判断**：OpenAPI 文档里 `security` 字段为空 / 没 `securitySchemes` 段。

### 2. 静态 token（Bearer / API Key）—— v0.1 推荐 Mode 5 替代

**方式 A：CLI flag（一次性）**

```bash
x serve --skill ./generated/api-skill --auth-bearer "$TOKEN"
# 或多个 header
x serve --skill ./generated/api-skill \
    --auth-header "X-API-Key=$API_KEY" \
    --auth-header "X-Tenant=$TENANT"
```

**方式 B：auth.yaml（持久化配置）** —— 推荐

```bash
cp ./generated/api-skill/auth.example.yaml ./generated/api-skill/auth.yaml
# 编辑 auth.yaml 填 token,然后:
x serve --skill ./generated/api-skill
```

详见 **Mode 5**。

### 3. 自动登录（JWT Bearer）—— v0.1 推荐 Mode 5

**适用**：后端用 JWT,login 端点不在 OpenAPI 里（Superset / GitLab / 自建网关风格）。

**方式 A：手工 curl + 环境变量**（旧模式,不推荐）

```bash
TOKEN=$(curl -s -X POST https://api.example.com/api/v1/security/login \
  -H "Content-Type: application/json" \
  -d '{"username":"admin","password":"xxx"}' \
  | jq -r .access_token)
x serve --skill ./generated/skill --auth-bearer "$TOKEN"
```

**问题**：401 过期得手工重启 serve + 重新拿 token。

**方式 B：auth.yaml 自动登录**（推荐）—— 详见 **Mode 5**。

### 4. API Key + Header（多 key 组合）

同 Mode 2,只是用 `--auth-header` 多次：

```bash
x serve --skill ./generated/api-skill \
    --auth-header "X-API-Key=$API_KEY" \
    --auth-header "X-Tenant=$TENANT"
```

或写到 `auth.yaml`：

```yaml
version: 1
token:
  kind: bearer
  bearer: "$API_KEY"  # 不推荐写明文,这里只是演示结构
```

### 5. auth.yaml session bootstrap（v0.1 推荐）

**最强模式**：x-cli serve 启动时**自动登录**拿 token,401 时**自动 re-login** + retry。**agent 不写一行鉴权代码**。

**使用流程**：

1. `x emit` 自动产出 `auth.example.yaml` 模板（in git）+ `.gitignore`
2. 用户 `cp auth.example.yaml auth.yaml`,填 login URL / body / creds
3. `x serve --skill <dir>` 启动时自动读 `auth.yaml` 并登录
4. 业务请求 401 → x-cli 自动 re-login + retry 一次
5. agent 只发 JSON-RPC,完全不知道有 auth 这回事

**auth.yaml schema**：

```yaml
version: 1
token:
  # 选项 A：静态 token(等价 --auth-bearer)
  # kind: bearer
  # bearer: "eyJhbGc..."
  
  # 选项 B：启动时自动登录(推荐)
  kind: login
  request:
    url: "https://api.example.com/api/v1/security/login"  # 缺省 = 用 skill base-url 拼 /login
    method: "POST"
    headers:
      Content-Type: "application/json"
    body:
      username: "admin"
      password: "REPLACE_ME"  # 明文(dev/demo 限定)
  response:
    token_path: "access_token"  # dotted path,默认 access_token
    expires_in_path: "expires_in"           # 可选,proactive refresh 用
    refresh_token_path: "refresh_token"     # 可选
  refresh:
    on_401: true     # 401 自动 re-login + retry,默认 true
    proactive: false # v0.1 暂不实现主动续(v0.2+)
```

**注意**：serde flatten 让 LoginConfig 字段内联到 `token:` 下,**没有 `login:` 包装**。这是最常踩的坑。

**完整端到端范例**：见 `examples/5-auth-yaml.md`。

## 选型决策表

| 现象 | 用哪种 |
|---|---|
| OpenAPI `securitySchemes.bearer` / `apiKey in header` | Mode 2 / 3 直接对应 → 推荐 Mode 5 |
| 后端 login 在 OpenAPI 之外(Superset 风格)| Mode 3 → **强烈推荐 Mode 5**(agent 不写鉴权代码)|
| 多租户 / 多 key 组合 | Mode 4 → Mode 5 + 多个 `--auth-header` 或 yaml 里多次声明 |
| 401 但 token 看起来对 | **检查 `--base-url`** —— 见 `troubleshooting.md` |
| 403 但权限该有的 | token 错或过期,Mode 5 自动 re-login;静态 token 重新拿 |
| Cookie + CSRF | v0.1 不支持,等 v0.2 |

## 排错速查

- **每次启 serve 都要带 auth** —— v0.1 不持久化到 skill 目录(`auth.yaml` gitignored,**有意设计**)
- **auth 参数不要写到 workflow.yaml 里** —— workflow.yaml 给 agent 读,硬编码 token 会进 git
- **测试时用环境变量**：

  ```bash
  read -s MY_TOKEN  # 不进 shell history
  x serve --skill ./generated/skill --auth-bearer "$MY_TOKEN"
  ```
- **Mode 5 的 401 透传是 fallback** —— re-login 一次还失败,业务请求会拿到 401,agent 报错给用户