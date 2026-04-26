# PingoraHub Systemd 部署

当前仓库提供的 `systemd + Rust 二进制` 部署模板位于：

- `deploy/systemd/pingorahub-hub-api.service`
- `deploy/systemd/pingorahub-dns-worker.service`
- `deploy/systemd/pingorahub-node-agent.service`
- `deploy/systemd/pingorahub-status-worker.service`
- `deploy/systemd/pingorahub-failover-worker.service`

默认部署布局：

- 二进制目录：`/opt/pingorahub/bin`
- 配置目录：`/etc/pingorahub`
- 数据/日志目录：`/var/lib/pingorahub`、`/var/log/pingorahub`

## 当前部署约定

- 当前约定为：本地只负责 `cargo build --release` 打包，不在开发机长期运行 `hub-api` 或其他 worker
- 远程部署目标主机：`10.20.3.53`
- SSH 连接账号：`root`
- SSH 端口：`22`
- 当前可直接连接方式：`ssh root@10.20.3.53`

如需发布新版本，建议流程为：

1. 在本地完成代码修改与测试
2. 在本地执行 `cargo build --release`
3. 将 `target/release/` 下的目标二进制同步到远程主机 `/opt/pingorahub/bin`
4. 将 `deploy/systemd/*.service` 与远程环境变量文件同步到远程主机
5. 在远程主机执行 `systemctl daemon-reload` 和对应服务重启

## 可直接执行的远程部署命令

下面命令默认以当前仓库根目录作为执行目录，目标机器固定为 `root@10.20.3.53`。

如果你希望直接走“一键发布”，仓库里已经提供：

```bash
bash scripts/deploy-remote.sh
```

它会自动完成：源码同步、远程编译、安装二进制、按文件名顺序应用全部未执行 migration、重载并重启已配置环境文件的 `hub-api / dns-worker / node-agent / status-worker / failover-worker`。

先在本地定义变量：

```bash
export PINGORAHUB_REMOTE_HOST=root@10.20.3.53
export PINGORAHUB_REMOTE_BIN_DIR=/opt/pingorahub/bin
export PINGORAHUB_REMOTE_ETC_DIR=/etc/pingorahub
export PINGORAHUB_REMOTE_SYSTEMD_DIR=/etc/systemd/system
```

### 首次部署

1. 本地打包：

```bash
cargo build --release \
  -p pingorahub-hub-api \
  -p pingorahub-dns-worker \
  -p pingorahub-node-agent \
  -p pingorahub-status-worker \
  -p pingorahub-failover-worker
```

2. 远程初始化目录、用户和权限：

```bash
ssh "$PINGORAHUB_REMOTE_HOST" '
  id pingorahub >/dev/null 2>&1 || useradd --system --home /opt/pingorahub --shell /usr/sbin/nologin pingorahub
  install -d -o pingorahub -g pingorahub /opt/pingorahub
  install -d -o pingorahub -g pingorahub /opt/pingorahub/bin
  install -d -o pingorahub -g pingorahub /etc/pingorahub
  install -d -o pingorahub -g pingorahub /var/lib/pingorahub
  install -d -o pingorahub -g pingorahub /var/log/pingorahub
'
```

3. 同步二进制和 `systemd` 文件：

```bash
rsync -avz \
  target/release/pingorahub-hub-api \
  target/release/pingorahub-dns-worker \
  target/release/pingorahub-node-agent \
  target/release/pingorahub-status-worker \
  target/release/pingorahub-failover-worker \
  "$PINGORAHUB_REMOTE_HOST:$PINGORAHUB_REMOTE_BIN_DIR/"

rsync -avz \
  deploy/systemd/pingorahub-hub-api.service \
  deploy/systemd/pingorahub-dns-worker.service \
  deploy/systemd/pingorahub-node-agent.service \
  deploy/systemd/pingorahub-status-worker.service \
  deploy/systemd/pingorahub-failover-worker.service \
  "$PINGORAHUB_REMOTE_HOST:$PINGORAHUB_REMOTE_SYSTEMD_DIR/"
```

4. 在远程创建环境变量文件：

`hub-api` 最小示例：

```bash
ssh "$PINGORAHUB_REMOTE_HOST" "cat > $PINGORAHUB_REMOTE_ETC_DIR/hub-api.env" <<'EOF'
PINGORAHUB_BIND=0.0.0.0:3000
PINGORAHUB_STORAGE_BACKEND=postgres_redis
PINGORAHUB_POSTGRES_URL=postgres://postgres:postgres@127.0.0.1:5432/pingorahub
PINGORAHUB_REDIS_URL=redis://127.0.0.1:6379
PINGORAHUB_REDIS_KEY_PREFIX=pingorahub
PINGORAHUB_BOOTSTRAP_ADMIN_USERNAME=admin
PINGORAHUB_BOOTSTRAP_ADMIN_PASSWORD=penkai
PINGORAHUB_BOOTSTRAP_ADMIN_DISPLAY_NAME=平台管理员
EOF
```

`status-worker` 示例：

```bash
ssh "$PINGORAHUB_REMOTE_HOST" "cat > $PINGORAHUB_REMOTE_ETC_DIR/status-worker.env" <<'EOF'
PINGORAHUB_POSTGRES_URL=postgres://postgres:postgres@127.0.0.1:5432/pingorahub
PINGORAHUB_POSTGRES_MAX_CONNECTIONS=5
PINGORAHUB_STATUS_WORKER_INTERVAL_SECONDS=15
PINGORAHUB_STATUS_WORKER_EXPECTED_HEARTBEAT_SECONDS=15
PINGORAHUB_STATUS_WORKER_SUSPECT_AFTER_MISSED=2
PINGORAHUB_STATUS_WORKER_OFFLINE_AFTER_MISSED=4
EOF
```

`failover-worker` 示例：

```bash
ssh "$PINGORAHUB_REMOTE_HOST" "cat > $PINGORAHUB_REMOTE_ETC_DIR/failover-worker.env" <<'EOF'
PINGORAHUB_POSTGRES_URL=postgres://postgres:postgres@127.0.0.1:5432/pingorahub
PINGORAHUB_POSTGRES_MAX_CONNECTIONS=5
PINGORAHUB_FAILOVER_WORKER_INTERVAL_SECONDS=15
PINGORAHUB_FAILOVER_WORKER_EXPECTED_HEARTBEAT_SECONDS=15
EOF
```

`dns-worker` 示例：

```bash
ssh "$PINGORAHUB_REMOTE_HOST" "cat > $PINGORAHUB_REMOTE_ETC_DIR/dns-worker.env" <<'EOF'
PINGORAHUB_POSTGRES_URL=postgres://postgres:postgres@127.0.0.1:5432/pingorahub
PINGORAHUB_POSTGRES_MAX_CONNECTIONS=5
PINGORAHUB_ACME_MODE=mock
PINGORAHUB_DNS_WORKER_BATCH_SIZE=10
PINGORAHUB_DNS_WORKER_INTERVAL_SECONDS=15
EOF
```

`node-agent` 示例：

```bash
ssh "$PINGORAHUB_REMOTE_HOST" "cat > $PINGORAHUB_REMOTE_ETC_DIR/node-agent.env" <<'EOF'
PINGORAHUB_HUB_BASE_URL=http://10.20.3.53:3000
PINGORAHUB_NODE_AGENT_DATA_DIR=/var/lib/pingorahub/node-agent
PINGORAHUB_NODE_REGISTRATION_PATH=/etc/pingorahub/manual-node-registration.json
PINGORAHUB_NODE_AGENT_MIN_HEARTBEAT_SECONDS=15
EOF
```

5. 首次初始化数据库时，按顺序执行迁移：

```bash
ssh "$PINGORAHUB_REMOTE_HOST" '
  psql postgres://postgres:postgres@127.0.0.1:5432/pingorahub -f /path/to/PingoraMesh/migrations/0001_init.sql
  psql postgres://postgres:postgres@127.0.0.1:5432/pingorahub -f /path/to/PingoraMesh/migrations/0002_admin_auth.sql
  psql postgres://postgres:postgres@127.0.0.1:5432/pingorahub -f /path/to/PingoraMesh/migrations/0003_runtime_fixes.sql
'
```

如果远程机器上没有仓库源码，可以先把 `migrations/` 目录同步过去：

```bash
rsync -avz migrations/ "$PINGORAHUB_REMOTE_HOST:/opt/pingorahub/migrations/"
```

然后把上面的 `/path/to/PingoraMesh/migrations/...` 改成 `/opt/pingorahub/migrations/...`。

6. 远程加载并启动服务：

```bash
ssh "$PINGORAHUB_REMOTE_HOST" '
  systemctl daemon-reload
  systemctl enable pingorahub-hub-api
  systemctl restart pingorahub-hub-api
  systemctl enable pingorahub-status-worker
  systemctl restart pingorahub-status-worker
  systemctl enable pingorahub-failover-worker
  systemctl restart pingorahub-failover-worker
  systemctl enable pingorahub-dns-worker
  systemctl restart pingorahub-dns-worker
'
```

7. 远程检查状态：

```bash
ssh "$PINGORAHUB_REMOTE_HOST" '
  systemctl --no-pager --full status pingorahub-hub-api
  systemctl --no-pager --full status pingorahub-status-worker
  systemctl --no-pager --full status pingorahub-failover-worker
  systemctl --no-pager --full status pingorahub-dns-worker
'

curl http://10.20.3.53:3000/healthz
```

### 日常发版

如果只是更新二进制并重启远程服务，可以直接执行：

```bash
cargo build --release -p pingorahub-hub-api

rsync -avz \
  target/release/pingorahub-hub-api \
  "$PINGORAHUB_REMOTE_HOST:$PINGORAHUB_REMOTE_BIN_DIR/"

ssh "$PINGORAHUB_REMOTE_HOST" '
  systemctl restart pingorahub-hub-api
  systemctl --no-pager --full status pingorahub-hub-api
'

curl http://10.20.3.53:3000/healthz
```

如果本次修改涉及 worker，同理替换对应二进制和服务名即可。

建议服务：

- `pingorahub-hub-api`
- `pingorahub-dns-worker`
- `pingorahub-node-agent`
- `pingorahub-status-worker`
- `pingorahub-failover-worker`

当前推荐运行模式：

- `hub-api`：`postgres_redis`
- `dns-worker`：连接本机 PostgreSQL，`ACME` 可先用 `mock`，再切到 `letsencrypt-staging`

当前代码实现边界：

- `hub-api` 已可对外提供 HTTP API 与内置控制台
- `dns-worker` 已支持证书订单状态推进
- `node-agent` 已支持 `refresh token + heartbeat + pull latest release + apply pipeline + node_operations + ACK`
- `status-worker` 已支持基于 `last_seen_at` 的节点状态回收
- `failover-worker` 已支持基于 `failover_policies` 的最小自动切换、failover 发布与 post-switch node operations
- `release-worker` 仍偏骨架，暂不建议作为正式生产服务一起托管

部署完成后的默认访问方式：

- 控制台：`http://<server-ip>:3000/`
- 健康检查：`http://<server-ip>:3000/healthz`
- 管理端元信息：`http://<server-ip>:3000/api/admin/meta`

当前控制台模块：

- 概览
- 节点
- 站点
- 发布
- DNS
- 证书
