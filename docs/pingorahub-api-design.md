# PingoraHub API 详细设计

## 1. 设计目标

本设计用于把 PingoraHub 方案进一步细化到接口层，目标是为前端控制台、节点 Agent、后续 OpenAPI 文档和 Rust Handler 实现提供统一契约。

## 2. 接口约定

### 2.1 基础约定

- Base Path：`/api`
- 数据格式：`application/json`
- 时间格式：`RFC3339`
- ID 类型：`UUID`
- 版本策略：第一阶段先使用 URI 固定版本，不额外引入 `/v1`

### 2.2 鉴权模型

管理端：

- `Authorization: Bearer <jwt>`
- 支持接入 OIDC 或平台内部签发 JWT

节点端：

- 首次注册使用 `node_code + bootstrap_token`
- 注册成功后使用 `Bearer node_access_token`
- 安全要求更高时可升级为 `mTLS + token`

### 2.3 通用响应格式

成功响应：

```json
{
  "request_id": "9d1497d0-1a46-4d72-a27f-f77379a88c1d",
  "data": {},
  "meta": {}
}
```

错误响应：

```json
{
  "request_id": "9d1497d0-1a46-4d72-a27f-f77379a88c1d",
  "error": {
    "code": "site_not_found",
    "message": "site does not exist",
    "details": {}
  }
}
```

### 2.4 幂等要求

以下接口建议支持 `Idempotency-Key`：

- 创建站点
- 创建发布
- 手动触发故障切换
- 创建节点运维动作
- DNS 记录变更
- 证书申请

## 3. 管理端 API

## 3.1 节点管理

### `POST /api/admin/nodes`

用途：

- 创建节点档案
- 生成初始化引导信息

请求体：

```json
{
  "node_code": "cn-sh-01",
  "name": "上海主入口 01",
  "region": "cn-east",
  "idc": "sh-telecom-a",
  "labels": {
    "role": "edge",
    "isp": "telecom"
  }
}
```

响应重点字段：

- `node_id`
- `bootstrap_token`
- `expires_at`

### `GET /api/admin/nodes`

用途：

- 按状态、地域、标签过滤节点
- 查看当前在线状态和生效版本

查询参数：

- `status`
- `region`
- `label_key`
- `label_value`

### `POST /api/admin/nodes/{node_id}/operations`

用途：

- 向节点派发模板化运维动作

请求体：

```json
{
  "template_id": "5e8730c0-82a6-4c1f-8a93-88848866bc67",
  "input_params": {
    "vip": "10.10.10.10/32",
    "iface": "eth1"
  },
  "approval_ticket": "OPS-20260410-001"
}
```

响应重点字段：

- `operation_id`
- `exec_status`

## 3.2 站点管理

### `POST /api/admin/sites`

用途：

- 创建站点及初始配置

请求体：

```json
{
  "site_code": "portal-prod",
  "name": "portal-prod",
  "domain": "portal.example.com",
  "listen_port": 443,
  "protocol": "https",
  "tls_enabled": true,
  "config": {
    "upstreams": [
      {
        "name": "portal-origin",
        "endpoints": [
          "10.0.1.10:8443",
          "10.0.1.11:8443"
        ]
      }
    ]
  }
}
```

### `PUT /api/admin/sites/{site_id}`

用途：

- 修改站点配置并生成新草稿版本
- 每次更新都会递增 `site_config.version`

### `POST /api/admin/sites/{site_id}/bindings`

用途：

- 绑定主节点与备用节点

请求体：

```json
{
  "bindings": [
    {
      "node_id": "3616107e-a7bf-4dca-bdd0-0830f4ed4f91",
      "binding_role": "primary",
      "priority": 10
    },
    {
      "node_id": "9f7ea481-246d-4859-a0a1-d29160ad52d5",
      "binding_role": "standby",
      "priority": 100
    }
  ]
}
```

### `DELETE /api/admin/sites/{site_id}/bindings/{node_id}`

用途：

- 解绑站点与节点

## 3.3 证书与 DNS

### `POST /api/admin/dns/providers`

用途：

- 接入新的 DNS Provider 账号

请求体：

```json
{
  "name": "cloudflare-prod",
  "provider_type": "cloudflare",
  "api_endpoint": "https://api.cloudflare.com/client/v4",
  "credentials": {
    "api_token": "******"
  }
}
```

### `POST /api/admin/dns/zones/sync`

用途：

- 从 Provider 拉取 Zone 列表并入库
- 第一版实现支持通过 `zone_names` 直接提交要同步的 Zone 名称

请求体：

```json
{
  "provider_id": "e2a8d23d-9514-4861-aaf1-e3ab79d5ef84",
  "zone_names": ["example.com"]
}
```

### `GET /api/admin/dns/providers`

用途：

- 查看已接入 DNS Provider 列表

### `GET /api/admin/dns/zones`

用途：

- 查看已同步的 Zone 列表

### `POST /api/admin/dns/records`

用途：

- 创建或修改 DNS 记录
- 支持 A/AAAA/CNAME/TXT

请求体：

```json
{
  "zone_id": "43384a7a-48b8-48fc-8cb4-160acdf5c4cf",
  "record_type": "A",
  "host": "portal",
  "value": "1.2.3.4",
  "ttl": 60
}
```

### `POST /api/admin/certificates`

用途：

- 手工导入证书

### `POST /api/admin/certificates/orders`

用途：

- 发起 ACME 自动签发或续期任务

请求体：

```json
{
  "site_id": "1fdf9207-d50e-46d4-b0df-5d8518972e90",
  "acme_provider": "letsencrypt",
  "challenge_type": "dns-01",
  "zone_id": "43384a7a-48b8-48fc-8cb4-160acdf5c4cf"
}
```

响应重点字段：

- `order_id`
- `order_status`

第一版实现说明：

- 当前会生成 DNS-01 挑战信息
- 当前会创建一个 `staging` 状态的证书占位记录
- 当前控制台可对失败订单执行 `retry`，对卡住的非成功订单执行 `reset`

### `GET /api/admin/certificates/orders`

用途：

- 查看证书申请任务和挑战信息

响应重点字段：

- `order_status`
- `challenge_payload`
- `error_message`

### `POST /api/admin/certificates/orders/{order_id}/retry`

用途：

- 将失败的证书订单重新排回 `pending_dns_challenge`

说明：

- 仅允许 `dns_challenge_failed`、`issue_failed`
- 会清空旧的 ACME challenge payload 和错误信息，下一轮 worker 会重新生成 challenge

### `POST /api/admin/certificates/orders/{order_id}/reset`

用途：

- 人工重置卡住的证书订单

说明：

- 适用于 `pending_dns_challenge`、`dns_challenge_presenting`、`dns_challenge_presented`、`issuing`、失败态
- 不允许对 `issued` 订单执行 reset
- reset 后会重新回到 `pending_dns_challenge`

## 3.4 发布管理

### `POST /api/admin/releases`

用途：

- 对站点或节点触发配置发布

请求体：

```json
{
  "scope_type": "site",
  "scope_id": "1fdf9207-d50e-46d4-b0df-5d8518972e90",
  "release_type": "publish",
  "reason": "update upstream weights"
}
```

响应重点字段：

- `release_id`
- `release_version`
- `targets`

### `GET /api/admin/releases`

用途：

- 查看发布列表
- 聚合 `pending / in_progress / success / failed` 状态

### `POST /api/admin/releases/{release_id}/rollback`

用途：

- 将指定发布回滚到上一稳定版本

## 3.5 故障切换

### `POST /api/admin/failover/policies`

用途：

- 定义主备节点关系与切换阈值

请求体：

```json
{
  "scope_type": "site",
  "scope_id": "1fdf9207-d50e-46d4-b0df-5d8518972e90",
  "primary_node_id": "3616107e-a7bf-4dca-bdd0-0830f4ed4f91",
  "standby_node_id": "9f7ea481-246d-4859-a0a1-d29160ad52d5",
  "trigger_mode": "semi_auto",
  "failure_threshold": 3,
  "recover_threshold": 5,
  "precheck_policy": {
    "require_standby_online": true,
    "require_config_prewarm": true
  }
}
```

### `POST /api/admin/failover/trigger`

用途：

- 手工触发切换或回切

请求体：

```json
{
  "policy_id": "66767f5e-606b-4516-8fbb-d44db4af2280",
  "action": "switch",
  "reason": "primary node offline"
}
```

## 4. 节点端 API

## 4.1 首次注册

### `POST /api/node/register`

请求体：

```json
{
  "node_code": "cn-sh-01",
  "bootstrap_token": "plain-token-from-admin",
  "hostname": "pingora-edge-01",
  "public_ip": "1.1.1.1",
  "private_ip": "10.0.0.10",
  "agent_version": "0.1.0"
}
```

响应体：

```json
{
  "request_id": "385053b9-3a5c-4916-a9c1-11b15f4cd69f",
  "data": {
    "node_id": "3616107e-a7bf-4dca-bdd0-0830f4ed4f91",
    "access_token": "node-access-token",
    "refresh_token": "node-refresh-token",
    "expires_at": "2026-04-10T15:10:00Z"
  }
}
```

## 4.2 心跳上报

### `POST /api/node/heartbeat`

鉴权要求：

- `Authorization: Bearer <node_access_token>`

请求体：

```json
{
  "node_id": "3616107e-a7bf-4dca-bdd0-0830f4ed4f91",
  "pingora_version": "0.5.0",
  "agent_version": "0.1.0",
  "active_config_version": "rel-20260410-001",
  "site_count": 12,
  "cpu_usage": 35.2,
  "mem_usage": 61.5,
  "disk_usage": 42.1,
  "health_score": 96
}
```

响应重点字段：

- `server_time`
- `next_heartbeat_after_seconds`

## 4.3 配置感知与拉取

### `GET /api/node/config/releases/latest`

用途：

- 节点在 etcd watch 触发后查询自身最新发布版本

鉴权要求：

- `Authorization: Bearer <node_access_token>`

响应体：

```json
{
  "request_id": "430aa1a3-2668-4f46-a80d-89ab5153ddf3",
  "data": {
    "release_id": "6c85af50-f7af-4d92-b2e9-baf3ebf41ea6",
    "release_version": "rel-20260410-001",
    "config_hash": "sha256:abcd",
    "download_url": "/api/node/config/package/rel-20260410-001"
  }
}
```

### `GET /api/node/config/package/{version}`

用途：

- 拉取节点专属配置包
- 返回配置、证书引用和操作指令

鉴权要求：

- `Authorization: Bearer <node_access_token>`

响应重点字段：

- `manifest`
- `rendered_config`
- `certificates`
- `signature`

## 4.4 发布 ACK

### `POST /api/node/releases/{release_id}/ack`

鉴权要求：

- `Authorization: Bearer <node_access_token>`

请求体：

```json
{
  "node_id": "3616107e-a7bf-4dca-bdd0-0830f4ed4f91",
  "apply_status": "success",
  "current_version": "rel-20260410-001",
  "message": "reloaded successfully"
}
```

## 4.5 运行状态上报

### `POST /api/node/status/report`

用途：

- 上报较高维度的运行状态、错误摘要、上游健康信息

## 4.6 节点运维动作结果回传

### `POST /api/node/operations/{operation_id}/result`

请求体：

```json
{
  "node_id": "9f7ea481-246d-4859-a0a1-d29160ad52d5",
  "exec_status": "success",
  "exit_code": 0,
  "stdout": "vip bound on eth1",
  "stderr": "",
  "finished_at": "2026-04-10T15:12:00Z"
}
```

## 4.7 节点 Token 刷新

### `POST /api/node/auth/refresh`

用途：

- 使用 `refresh_token` 刷新新的 `access_token`

## 5. 状态机建议

### 5.1 节点状态

- `pending`：已录入但未完成注册
- `online`：心跳正常
- `suspect`：连续丢失少量心跳
- `offline`：达到离线阈值
- `maintenance`：人工维护中

### 5.2 发布状态

- `draft`
- `pending`
- `publishing`
- `success`
- `failed`
- `rolled_back`

### 5.3 运维动作状态

- `pending`
- `approved`
- `dispatching`
- `running`
- `success`
- `failed`
- `cancelled`
- `timeout`

## 6. 第一阶段接口实现优先级

P0：

- `POST /api/admin/nodes`
- `POST /api/node/register`
- `POST /api/node/heartbeat`
- `POST /api/admin/sites`
- `POST /api/admin/sites/{site_id}/bindings`
- `POST /api/admin/releases`
- `GET /api/node/config/releases/latest`
- `GET /api/node/config/package/{version}`
- `POST /api/node/releases/{release_id}/ack`

P1：

- `POST /api/admin/dns/providers`
- `POST /api/admin/dns/zones/sync`
- `POST /api/admin/dns/records`
- `POST /api/admin/certificates/orders`
- `POST /api/admin/failover/policies`
- `POST /api/admin/nodes/{node_id}/operations`
- `POST /api/node/operations/{operation_id}/result`
