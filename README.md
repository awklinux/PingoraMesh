# PingoraMesh

PingoraMesh 是 PingoraHub 的实现仓库，目标是提供一个基于 Rust 的集中式 Pingora 节点管理平台，用于：

- 管理 Pingora 节点注册、认证、心跳与运行状态
- 管理站点配置、节点绑定与配置发布
- 管理证书、DNS 解析与 ACME 自动签发
- 管理主备节点切换与模板化运维动作

## 当前仓库内容

- 总体方案：[docs/pingorahub-solution.md](/Users/kevin/kevin/code/新钛云服/PingoraMesh/docs/pingorahub-solution.md)
- API 详细设计：[docs/pingorahub-api-design.md](/Users/kevin/kevin/code/新钛云服/PingoraMesh/docs/pingorahub-api-design.md)
- 第一阶段任务拆分：[docs/pingorahub-phase1-plan.md](/Users/kevin/kevin/code/新钛云服/PingoraMesh/docs/pingorahub-phase1-plan.md)
- 初始化数据库迁移：[migrations/0001_init.sql](/Users/kevin/kevin/code/新钛云服/PingoraMesh/migrations/0001_init.sql)
- 本地联调说明：[local-dev.md](/Users/kevin/kevin/code/新钛云服/PingoraMesh/docs/local-dev.md)

## Workspace 结构

```text
apps/
  hub-api/          管理端 + 节点端统一 API
  release-worker/   发布编排任务
  status-worker/    状态聚合任务
  dns-worker/       DNS 同步与证书续期任务
  failover-worker/  故障切换与运维动作任务
  node-agent/       节点守护进程骨架
crates/
  domain/           领域模型
  application/      应用服务和用例
  infrastructure/   配置与基础设施抽象
  protocol/         API DTO
  config-compiler/  配置编译器
  dns-provider/     DNS Provider 适配抽象
  ops-orchestrator/ 运维动作模板编排
```

## 快速开始

```bash
cargo check
cargo fmt
cargo run -p pingorahub-hub-api
```

默认会启动一个最小 `hub-api` 骨架，并暴露：

- `GET /` 控制台页面
- `GET /healthz`
- `GET /api/admin/meta`
- `GET /api/node/config/releases/latest`

控制台默认和 `hub-api` 同进程提供，适合直接通过 IP 访问，例如：

```bash
http://127.0.0.1:3000/
```

当前控制台已经支持：

- 概览面板与集群状态统计
- 节点创建与运行状态查看
- 站点创建、编辑、节点绑定
- 发布任务创建与状态聚合
- DNS Provider 接入与 Zone 同步
- 证书订单创建、重试、重置

## 存储后端

默认使用内存后端，适合本地开发和接口联调：

```bash
cargo run -p pingorahub-hub-api
```

如需切换到 `PostgreSQL + Redis`：

```bash
export PINGORAHUB_STORAGE_BACKEND=postgres_redis
export PINGORAHUB_POSTGRES_URL=postgres://postgres:postgres@127.0.0.1:5432/pingorahub
export PINGORAHUB_REDIS_URL=redis://127.0.0.1:6379
export PINGORAHUB_REDIS_KEY_PREFIX=pingorahub

cargo run -p pingorahub-hub-api
```

本地依赖环境可以直接使用：

```bash
docker compose -f deploy/docker-compose.local.yml up -d
```

## DNS Worker

`dns-worker` 现在支持两种运行模式：

- `provider demo`：直接用环境变量加载某个 DNS Provider，适合本地验证 zone 拉取
- `certificate order mode`：连接 PostgreSQL，轮询 `certificate_orders`，先处理 `pending_dns_challenge` 任务下发 DNS-01 TXT 记录，再把 `dns_challenge_presented` 任务推进到 ACME finalize、证书版本落库和站点证书绑定

证书订单模式示例：

```bash
export PINGORAHUB_POSTGRES_URL=postgres://postgres:postgres@127.0.0.1:5432/pingorahub
export PINGORAHUB_POSTGRES_MAX_CONNECTIONS=5
export PINGORAHUB_ACME_MODE=mock
export PINGORAHUB_DNS_WORKER_BATCH_SIZE=10
export PINGORAHUB_DNS_WORKER_INTERVAL_SECONDS=15
export PINGORAHUB_DNS_WORKER_ONCE=true

cargo run -p pingorahub-dns-worker
```

支持的 `ACME mode`：

- `mock`
- `letsencrypt-staging`
- `letsencrypt-production`
- `custom`

真实 ACME 模式需要额外环境变量：

```bash
export PINGORAHUB_ACME_MODE=letsencrypt-staging
export PINGORAHUB_ACME_ACCOUNT_KEY_PEM_PATH=/absolute/path/account-key.pem
export PINGORAHUB_ACME_CONTACT_EMAIL=ops@example.com
export PINGORAHUB_ACME_CERT_KEY_PASSPHRASE=change-me
export PINGORAHUB_ACME_DNS_PROPAGATION_WAIT_SECONDS=15
export PINGORAHUB_ACME_POLL_INTERVAL_SECONDS=5
export PINGORAHUB_ACME_POLL_TIMEOUT_SECONDS=180
```

如使用 `custom`，再额外提供：

```bash
export PINGORAHUB_ACME_DIRECTORY_URL=https://your-acme-server/directory
```

`mock` 模式适合本地联调，会生成一份 mock 证书材料写入 `certificate_versions` 与 `site_cert_bindings`。`letsencrypt-staging` 适合先打通真实流程，再切到生产目录。

当前真实 ACME 实现先按单域名链路落地，只支持单个 DNS identifier 的订单。

如需运行 `PostgreSQL + Redis` 集成测试：

```bash
export PINGORAHUB_TEST_POSTGRES_URL=postgres://postgres:postgres@127.0.0.1:5432/pingorahub
export PINGORAHUB_TEST_REDIS_URL=redis://127.0.0.1:6379

cargo test -p pingorahub-hub-api postgres_redis_integration_flow -- --ignored
```

## Node Agent Apply Pipeline

`node-agent` 现在支持更接近真实发布的执行链路：

- 先把发布包落到 `releases/<release_version>/`
- 先回传 `downloading` / `applying`
- 可选执行外部应用命令，例如重载 Pingora 或 Nginx
- 轮询 `/api/node/operations/pending` 并执行模板化运维动作
- 将运维动作结果回传到 `/api/node/operations/{operation_id}/result`
- 可选执行 HTTP 健康检查
- 只有应用动作和健康检查都通过后，才回传 `success`

常用环境变量：

```bash
export PINGORAHUB_NODE_APPLY_COMMAND='pingora -c "$PINGORAHUB_RELEASE_RENDERED_CONFIG_PATH" --test && systemctl reload pingora'
export PINGORAHUB_NODE_APPLY_TIMEOUT_SECONDS=30
export PINGORAHUB_NODE_APPLY_HEALTHCHECK_URL=http://127.0.0.1:8080/healthz
export PINGORAHUB_NODE_APPLY_HEALTHCHECK_EXPECT_STATUS=200
export PINGORAHUB_NODE_APPLY_HEALTHCHECK_TIMEOUT_SECONDS=10
export PINGORAHUB_NODE_APPLY_HEALTHCHECK_RETRIES=3
export PINGORAHUB_NODE_APPLY_HEALTHCHECK_INTERVAL_SECONDS=2
```

默认要求显式配置 `PINGORAHUB_NODE_APPLY_COMMAND`，否则 `node-agent` 不会把发布 ACK 为 `success`。如需在本地演示或空跑环境跳过真实应用动作，可显式设置：

```bash
export PINGORAHUB_NODE_ALLOW_NOOP_APPLY=true
```

应用命令执行时会注入这些上下文变量：

- `PINGORAHUB_RELEASE_DIR`
- `PINGORAHUB_RELEASE_PACKAGE_PATH`
- `PINGORAHUB_RELEASE_MANIFEST_PATH`
- `PINGORAHUB_RELEASE_RENDERED_CONFIG_PATH`
- `PINGORAHUB_RELEASE_CERTIFICATES_PATH`
- `PINGORAHUB_RELEASE_VERSION`
- `PINGORAHUB_PREVIOUS_RELEASE_VERSION`

## Status Worker

`status-worker` 现在会周期性扫描 `nodes.last_seen_at`，按心跳缺失次数把节点回收到：

- `online`
- `suspect`
- `offline`

默认参数：

- 期望心跳间隔：`15s`
- `suspect_after_missed=2`
- `offline_after_missed=4`

常用环境变量：

```bash
export PINGORAHUB_POSTGRES_URL=postgres://postgres:postgres@127.0.0.1:5432/pingorahub
export PINGORAHUB_POSTGRES_MAX_CONNECTIONS=5
export PINGORAHUB_STATUS_WORKER_INTERVAL_SECONDS=15
export PINGORAHUB_STATUS_WORKER_EXPECTED_HEARTBEAT_SECONDS=15
export PINGORAHUB_STATUS_WORKER_SUSPECT_AFTER_MISSED=2
export PINGORAHUB_STATUS_WORKER_OFFLINE_AFTER_MISSED=4
export PINGORAHUB_STATUS_WORKER_ONCE=true

cargo run -p pingorahub-status-worker
```

## Failover Worker

`failover-worker` 现在支持最小可运行的站点主备切换链路：

- 扫描 `active` 的 `failover_policies`
- 对 `site` 范围的 `semi_auto / auto` 策略做自动判定
- 当主节点达到故障阈值且备用节点满足预检条件时，切换站点主备绑定
- 自动生成一条 `failover` 类型发布，把配置重新下发到新的承载节点
- 如果策略里配置了 `post_switch_operations`，自动向目标节点入队 `node_operations`
- 记录 `failover_events` 和审计日志

当前边界：

- 已支持自动“切到备用节点”
- 还未实现自动“回切主节点”
- `node_operations` 当前只支持“模板化命令 -> 节点执行 -> 结果回传”
- `run_as_user` 当前要求与 `node-agent` 进程用户一致，不做提权切换

常用环境变量：

```bash
export PINGORAHUB_POSTGRES_URL=postgres://postgres:postgres@127.0.0.1:5432/pingorahub
export PINGORAHUB_POSTGRES_MAX_CONNECTIONS=5
export PINGORAHUB_FAILOVER_WORKER_INTERVAL_SECONDS=15
export PINGORAHUB_FAILOVER_WORKER_EXPECTED_HEARTBEAT_SECONDS=15
export PINGORAHUB_FAILOVER_WORKER_ONCE=true

cargo run -p pingorahub-failover-worker
```
