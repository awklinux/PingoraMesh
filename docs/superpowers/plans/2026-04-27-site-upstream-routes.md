# Site Upstream Routes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Add Nginx-like path route rules so one site can forward different paths to different upstream groups.

**Architecture:** Extend the domain `SiteSpec` with `routes`, parse and validate `routes` from site JSON, publish routes in release packages, and add a console route editor next to the upstream editor. Missing `routes` on legacy configs normalize to a default `/` route pointing to the first upstream.

**Tech Stack:** Rust domain/application code, SQL-backed JSON site config, config compiler release bundles, vanilla JS console UI.

---

### Task 1: Domain And Compiler Routes

**Files:**
- Modify: `crates/domain/src/site.rs`
- Modify: `crates/domain/src/lib.rs`
- Modify: `crates/config-compiler/src/lib.rs`

- [x] **Step 1: Write failing compiler test**

Add a test that creates a `SiteSpec` with two `SiteRoute` entries and asserts `rendered_config.routes[0].upstream == "api"`.

- [x] **Step 2: Run test to verify it fails**

Run: `cargo test -p pingorahub-config-compiler routes -- --nocapture`

Expected: compile failure because `SiteRoute` and `routes` do not exist.

- [x] **Step 3: Implement domain types**

Add `RouteMatchType` and `SiteRoute`, export them from the domain crate, and add `routes: Vec<SiteRoute>` to `SiteSpec` with `#[serde(default)]`.

- [x] **Step 4: Include routes in compiler output**

Add `"routes": site.routes` to `rendered_config`.

- [x] **Step 5: Run compiler test**

Run: `cargo test -p pingorahub-config-compiler routes -- --nocapture`

Expected: route compiler test passes.

### Task 2: Hub API Parsing, Validation, And Compatibility

**Files:**
- Modify: `apps/hub-api/src/store.rs`
- Modify: `apps/hub-api/src/service.rs`
- Modify: `apps/hub-api/src/main.rs`

- [x] **Step 1: Write failing hub-api tests**

Add tests for legacy default route generation, multiple routes in package output, and unknown upstream rejection.

- [x] **Step 2: Run tests to verify failure**

Run: `cargo test -p pingorahub-hub-api route -- --nocapture`

Expected: failure because route parsing and validation are missing.

- [x] **Step 3: Implement parser helpers**

Parse `config.routes`, normalize legacy missing routes, and reject explicit empty route arrays when upstreams exist.

- [x] **Step 4: Implement validation**

Validate path, match type, upstream reference, duplicate active route keys, and default `/` route.

- [x] **Step 5: Wire validation into site create/update and release**

Validate configs before saving and before compiling releases.

- [x] **Step 6: Run hub-api route tests**

Run: `cargo test -p pingorahub-hub-api route -- --nocapture`

Expected: route tests pass.

### Task 3: Failover Worker Route Preservation

**Files:**
- Modify: `apps/failover-worker/src/main.rs`

- [x] **Step 1: Write failing failover route test**

Add a unit test proving `parse_routes` returns a default legacy route and preserves configured route targets.

- [x] **Step 2: Run failover test to verify failure**

Run: `cargo test -p pingorahub-failover-worker routes -- --nocapture`

Expected: compile or assertion failure before implementation.

- [x] **Step 3: Implement route parsing in failover worker**

Add matching route parser helpers and pass parsed routes into failover `SiteSpec`.

- [x] **Step 4: Run failover route tests**

Run: `cargo test -p pingorahub-failover-worker routes -- --nocapture`

Expected: tests pass.

### Task 4: Console Route Editor

**Files:**
- Modify: `apps/hub-api/assets/console/index.html`
- Modify: `apps/hub-api/assets/console/app.js`
- Modify: `apps/hub-api/assets/console/styles.css`

- [x] **Step 1: Add managed route block**

Add `转发规则` UI below upstream groups with route name, match type, path, upstream select, priority, enabled, strip prefix, reorder, and delete controls.

- [x] **Step 2: Add route JSON collection and hydration**

Implement `collectManagedRoutes`, `applyManagedRoutesToForm`, default route generation, and upstream select refresh.

- [x] **Step 3: Verify JS syntax**

Run: `node --check apps/hub-api/assets/console/app.js`

Expected: exit 0.

### Task 5: Full Verification

**Files:**
- All modified files.

- [x] **Step 1: Format Rust**

Run: `cargo fmt`

Expected: exit 0.

- [x] **Step 2: Run targeted tests**

Run:
- `cargo test -p pingorahub-config-compiler routes -- --nocapture`
- `cargo test -p pingorahub-hub-api route -- --nocapture`
- `cargo test -p pingorahub-failover-worker routes -- --nocapture`

Expected: all pass.

- [x] **Step 3: Run full workspace tests**

Run: `cargo test --workspace`

Expected: all non-ignored tests pass.

- [x] **Step 4: Commit**

Commit message: `Add site route rules for upstream forwarding`.
