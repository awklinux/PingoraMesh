# Site Upstream Routes Design

## Background

PingoraMesh already lets a site define multiple `upstreams`. Each upstream can configure a balance method, ordered endpoints, weight, active state, and backup nodes. The missing piece is request routing: today the release package carries upstream groups, but it has no explicit rule that maps `/api`, `/static`, or `/` traffic to a chosen upstream.

This design adds an Nginx-like location layer to site configuration while keeping existing upstream configuration compatible.

## Goals

- Support multiple upstream groups per site.
- Support multiple forwarding rules per site, similar to Nginx `location /api`.
- Route different paths to different upstreams.
- Keep old site configs working without manual migration.
- Keep the first implementation simple and predictable: exact path and prefix path matching only.
- Make the control console easy to operate with list-style editing for upstream pools and route rules.

## Non-Goals

- No regular expression route matching in the first version.
- No host-based routing inside a single site; each site is already domain-scoped.
- No runtime traffic engine rewrite in this design document. The release package will expose the data needed by node-side apply logic.
- No database schema change is required because site config is already stored as JSON.

## Config Model

Site config gains a top-level `routes` array:

```json
{
  "upstreams": [
    {
      "name": "web",
      "balance_method": "round_robin",
      "endpoints": [
        {
          "address": "10.20.3.65:8088",
          "weight": 100,
          "active": true,
          "backup": false
        }
      ]
    },
    {
      "name": "api",
      "balance_method": "weighted_round_robin",
      "endpoints": [
        {
          "address": "10.20.3.68:9000",
          "weight": 100,
          "active": true,
          "backup": false
        }
      ]
    }
  ],
  "routes": [
    {
      "name": "api-route",
      "enabled": true,
      "match_type": "path_prefix",
      "path": "/api",
      "upstream": "api",
      "priority": 10,
      "strip_prefix": false
    },
    {
      "name": "default",
      "enabled": true,
      "match_type": "path_prefix",
      "path": "/",
      "upstream": "web",
      "priority": 1000,
      "strip_prefix": false
    }
  ]
}
```

## Domain Types

Add `SiteRoute` and `RouteMatchType` to `crates/domain/src/site.rs`.

```rust
pub enum RouteMatchType {
    PathPrefix,
    PathExact,
}

pub struct SiteRoute {
    pub name: String,
    pub enabled: bool,
    pub match_type: RouteMatchType,
    pub path: String,
    pub upstream: String,
    pub priority: i32,
    pub strip_prefix: bool,
}
```

`SiteSpec` gains:

```rust
pub routes: Vec<SiteRoute>
```

The field must use `#[serde(default)]` so older manifests and tests remain compatible.

## Matching Semantics

- Disabled routes are ignored.
- `path_exact` matches only the complete request path.
- `path_prefix` matches the route path and child paths.
- `/api` matches `/api` and `/api/users`, but not `/apix`.
- Routes are sorted by `priority ASC`.
- If priorities are equal, longer `path` wins.
- If still equal, `path_exact` wins over `path_prefix`.
- A site must have a default route for `/`.
- `strip_prefix` controls whether the matched prefix is removed before forwarding. It is stored and published now; node-side apply can decide how to translate it into the runtime proxy config.

## Validation

Hub API should validate site config before saving and publishing:

- Every route must have a non-empty `name`.
- Every route `path` must start with `/`.
- Every route `match_type` must be either `path_prefix` or `path_exact`.
- Every route `upstream` must reference an existing upstream name.
- Duplicate active routes with the same `match_type`, `path`, and `priority` are rejected.
- At least one active default route for `/` is required.
- If `routes` is missing and `upstreams` is non-empty, the service auto-generates one default route to the first upstream for backward compatibility.
- If `routes` is present but empty while `upstreams` is non-empty, the service treats that as invalid because the operator explicitly removed routes.

## Backward Compatibility

Existing configs like this continue to work:

```json
{
  "upstreams": [
    {
      "name": "origin",
      "endpoints": ["10.20.3.65:8088"]
    }
  ]
}
```

The service normalizes it in memory and release output as:

```json
{
  "routes": [
    {
      "name": "default",
      "enabled": true,
      "match_type": "path_prefix",
      "path": "/",
      "upstream": "origin",
      "priority": 1000,
      "strip_prefix": false
    }
  ]
}
```

This preserves existing sites and avoids requiring a one-time migration.

## Control Console

The site configuration page should have two managed blocks:

- `Upstreams 池`: current upstream group editor, including balance method, endpoint order, weight, active, and backup.
- `转发规则`: new route list editor.

Each route row contains:

- Route name.
- Match type: `前缀匹配` or `精确匹配`.
- Path, for example `/api`.
- Target upstream select populated from configured upstream groups.
- Priority number.
- Enable toggle.
- Strip prefix toggle.
- Actions: move up, move down, delete.

The JSON preview remains the source of truth and updates as operators edit either block.

## API And Compiler Flow

Hub API:

- Parse upstreams and routes from `site.config`.
- Normalize missing routes only for legacy configs.
- Validate route references before save and before release.
- Return route data in site detail so the console can edit existing routes.

Config compiler:

- Include `routes` in `rendered_config`.
- Include `routes` in `ReleaseManifest.sites`.

Failover worker:

- Parse routes the same way Hub API does when creating failover releases.
- Preserve route config during primary switching and DNS failover releases.

Node agent:

- No protocol change is required beyond receiving the new `routes` field.
- Existing apply command receives `PINGORAHUB_RELEASE_RENDERED_CONFIG_PATH`, so downstream runtime renderers can consume routes from the rendered config.

## Example Behavior

With these routes:

- `/api` -> `api`
- `/` -> `web`

Requests route as follows:

- `/api` forwards to `api`.
- `/api/users` forwards to `api`.
- `/apix` forwards to `web`.
- `/static/app.js` forwards to `web`.

## Tests

Add or update tests for:

- Domain serialization of `SiteRoute`.
- Legacy config without `routes` gets default `/` route.
- Multiple routes are preserved in rendered config and manifest.
- Unknown upstream reference is rejected.
- Duplicate active route keys are rejected.
- Prefix matching order sorts `/api/admin` ahead of `/api` when priorities tie.
- Control console JSON generation includes `routes`.
- Failover release preserves `routes`.

## Rollout Plan

1. Add domain route types and parser helpers.
2. Add route validation and legacy normalization in Hub API/store parsing.
3. Add compiler output for routes.
4. Update failover-worker route parsing.
5. Add console route editor and JSON preview integration.
6. Run targeted tests and `cargo test --workspace`.
7. Deploy to `10.20.3.53`.
8. Verify with a site such as `test001.feb.pub` using `/api -> api upstream` and `/ -> web upstream`.

## Risks And Mitigations

- Risk: route references drift after upstream rename.
  Mitigation: console should update route target selects from current upstream names, and API validation catches missing references.
- Risk: route order is misunderstood.
  Mitigation: show priority explicitly and sort rows by priority in the UI.
- Risk: old runtime apply logic ignores routes.
  Mitigation: publish routes in rendered config first; node-side apply can adopt them without changing Hub API again.
- Risk: no default route causes traffic drop.
  Mitigation: require default `/` route for explicit route configs and auto-generate it only for legacy configs.
