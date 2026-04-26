# PingoraHub 本地开发

## 启动本地依赖

```bash
cd /Users/kevin/kevin/code/新钛云服/PingoraMesh
docker compose -f deploy/docker-compose.local.yml up -d
```

默认会启动：

- PostgreSQL: `127.0.0.1:5432`
- Redis: `127.0.0.1:6379`
- etcd: `127.0.0.1:2379`

## 启动 hub-api

内存模式：

```bash
cargo run -p pingorahub-hub-api
```

## 运行 dns-worker

Provider demo 模式：

```bash
cargo run -p pingorahub-dns-worker
```

Cloudflare 适配器示例：

```bash
export PINGORAHUB_DNS_PROVIDER_TYPE=cloudflare
export PINGORAHUB_DNS_API_TOKEN=your_cloudflare_api_token
export PINGORAHUB_DNS_API_ENDPOINT=https://api.cloudflare.com/client/v4
export PINGORAHUB_DNS_ZONE_NAME=example.com

cargo run -p pingorahub-dns-worker
```

证书订单处理模式：

```bash
export PINGORAHUB_POSTGRES_URL=postgres://postgres:postgres@127.0.0.1:5432/pingorahub
export PINGORAHUB_POSTGRES_MAX_CONNECTIONS=5
export PINGORAHUB_ACME_MODE=mock
export PINGORAHUB_DNS_WORKER_BATCH_SIZE=10
export PINGORAHUB_DNS_WORKER_INTERVAL_SECONDS=15
export PINGORAHUB_DNS_WORKER_ONCE=true

cargo run -p pingorahub-dns-worker
```

这个模式会轮询 `certificate_orders`，处理两段状态：

- `pending_dns_challenge -> dns_challenge_presented`
- `dns_challenge_presented -> issued`

推荐先用 `mock` 模式把链路跑通，再切 `letsencrypt-staging`：

```bash
export PINGORAHUB_ACME_MODE=letsencrypt-staging
export PINGORAHUB_ACME_ACCOUNT_KEY_PEM_PATH=/absolute/path/account-key.pem
export PINGORAHUB_ACME_CONTACT_EMAIL=ops@example.com
export PINGORAHUB_ACME_CERT_KEY_PASSPHRASE=change-me
export PINGORAHUB_ACME_DNS_PROPAGATION_WAIT_SECONDS=15
export PINGORAHUB_ACME_POLL_INTERVAL_SECONDS=5
export PINGORAHUB_ACME_POLL_TIMEOUT_SECONDS=180

cargo run -p pingorahub-dns-worker
```

说明：

- `mock` 会生成联调用证书材料，并写入 `certificate_versions`、`certificates`、`site_cert_bindings`
- `letsencrypt-staging` 和 `letsencrypt-production` 会调用真实 ACME 目录
- `custom` 模式需要额外设置 `PINGORAHUB_ACME_DIRECTORY_URL`
- 当前真实 ACME 流程先只支持单个 DNS identifier 订单

持久化模式：

```bash
export PINGORAHUB_STORAGE_BACKEND=postgres_redis
export PINGORAHUB_POSTGRES_URL=postgres://postgres:postgres@127.0.0.1:5432/pingorahub
export PINGORAHUB_REDIS_URL=redis://127.0.0.1:6379
export PINGORAHUB_REDIS_KEY_PREFIX=pingorahub

cargo run -p pingorahub-hub-api
```

## 运行测试

普通测试：

```bash
cargo test -p pingorahub-hub-api
```

PostgreSQL + Redis 集成测试：

```bash
export PINGORAHUB_TEST_POSTGRES_URL=postgres://postgres:postgres@127.0.0.1:5432/pingorahub
export PINGORAHUB_TEST_REDIS_URL=redis://127.0.0.1:6379

cargo test -p pingorahub-hub-api postgres_redis_integration_flow -- --ignored
```

## 当前已实现的管理端接口

- `POST /api/admin/nodes`
- `GET /api/admin/nodes`
- `POST /api/admin/sites`
- `PUT /api/admin/sites/{site_id}`
- `GET /api/admin/sites`
- `POST /api/admin/sites/{site_id}/bindings`
- `POST /api/admin/releases`
- `GET /api/admin/releases`
- `POST /api/admin/dns/providers`
- `GET /api/admin/dns/providers`
- `POST /api/admin/dns/zones/sync`
- `GET /api/admin/dns/zones`
- `POST /api/admin/certificates/orders`
- `GET /api/admin/certificates/orders`
- `POST /api/admin/certificates/orders/{order_id}/retry`
- `POST /api/admin/certificates/orders/{order_id}/reset`

## 当前已实现的节点端接口

- `POST /api/node/register`
- `POST /api/node/auth/refresh`
- `POST /api/node/heartbeat`
- `GET /api/node/config/releases/latest`
- `GET /api/node/config/package/{version}`
- `POST /api/node/releases/{release_id}/ack`
