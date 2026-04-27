mod console;
mod error;
mod service;
mod store;
mod token_store;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
    routing::{delete, get, post, put},
};
use console::{
    console_iconfont, console_index, console_script, console_styles, login_index, login_script,
    login_styles,
};
use error::{ApiResult, AppError};
use pingorahub_infrastructure::HubConfig;
use pingorahub_protocol::{
    AdminLoginRequest, AdminMeResponse, ApiResponse, CertificateOrderOverview,
    ChangeAdminPasswordRequest, ChangeAdminPasswordResponse, ConfigPackageResponse,
    CreateCertificateOrderRequest, CreateCertificateOrderResponse, CreateDnsProviderRequest,
    CreateDnsProviderResponse, CreateNodeOperationRequest, CreateNodeRequest, CreateNodeResponse,
    CreateOperationTemplateRequest, CreateReleaseRequest, CreateReleaseResponse,
    CreateSiteBindingsRequest, CreateSiteBindingsResponse, CreateSiteRequest, CreateSiteResponse,
    DeleteDnsProviderResponse, DeleteDnsZoneResponse, DeleteNodeResponse,
    DeleteOperationTemplateResponse, DeleteSiteResponse, DnsProviderOverview, DnsZoneOverview,
    LatestReleaseResponse, NodeDetail, NodeHeartbeatRequest, NodeHeartbeatResponse,
    NodeOperationOverview, NodeOverview, NodeRefreshRequest, NodeRefreshResponse,
    NodeRegisterRequest, NodeRegisterResponse, OperationResultRequest, OperationResultResponse,
    OperationTemplateOverview, PendingOperationResponse, ReleaseAckRequest, ReleaseAckResponse,
    ReleaseOverview, RenewSiteCertificateResponse, SiteDetail, SiteOverview,
    SwitchSitePrimaryRequest, SwitchSitePrimaryResponse, SyncDnsZonesRequest, SyncDnsZonesResponse,
    UpdateDnsZoneStatusResponse, UpdateSiteRequest, UpdateSiteStatusResponse,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use service::HubService;
use tracing::info;
use uuid::Uuid;

const ADMIN_SESSION_COOKIE: &str = "pingorahub_admin_session";

#[derive(Clone)]
struct AppState {
    config: HubConfig,
    service: HubService,
}

#[derive(Debug, Serialize)]
struct HealthPayload {
    service: &'static str,
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct AdminMetaPayload {
    service: &'static str,
    bind: String,
    etcd_prefix: String,
    components: Vec<&'static str>,
}

#[derive(Debug, Deserialize)]
struct NodeScopedQuery {
    node_id: Uuid,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let mut config = HubConfig::default();
    if let Ok(bind) = std::env::var("PINGORAHUB_BIND") {
        config.bind = bind;
    }
    if let Ok(storage_backend) = std::env::var("PINGORAHUB_STORAGE_BACKEND") {
        config.storage.backend = storage_backend;
    }
    if let Ok(postgres_url) = std::env::var("PINGORAHUB_POSTGRES_URL") {
        config.postgres.url = postgres_url;
    }
    if let Ok(redis_url) = std::env::var("PINGORAHUB_REDIS_URL") {
        config.redis.url = redis_url;
    }
    if let Ok(redis_key_prefix) = std::env::var("PINGORAHUB_REDIS_KEY_PREFIX") {
        config.storage.redis_key_prefix = redis_key_prefix;
    }
    if let Ok(auto_renew_before_days) = std::env::var("PINGORAHUB_CERT_AUTO_RENEW_BEFORE_DAYS") {
        config.certificate.auto_renew_before_days =
            auto_renew_before_days.parse::<i64>().unwrap_or(30).max(1);
    }
    config.validate()?;

    let bind = config.bind.clone();
    let service = HubService::from_config(&config).await?;
    if let (Ok(username), Ok(password)) = (
        std::env::var("PINGORAHUB_BOOTSTRAP_ADMIN_USERNAME"),
        std::env::var("PINGORAHUB_BOOTSTRAP_ADMIN_PASSWORD"),
    ) {
        let display_name = std::env::var("PINGORAHUB_BOOTSTRAP_ADMIN_DISPLAY_NAME")
            .unwrap_or_else(|_| "Administrator".to_string());
        let created = service
            .ensure_admin_user(&username, &display_name, &password)
            .await
            .map_err(|error| anyhow::anyhow!(error.message.clone()))?;
        info!(username = %username, created, "bootstrap admin processed");
    }
    let state = AppState { config, service };
    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind(&bind).await?;
    info!(bind = %bind, "hub-api listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(console_entry))
        .route("/console", get(console_entry))
        .route("/console/", get(console_entry))
        .route("/console/assets/styles.css", get(console_styles))
        .route("/console/assets/app.js", get(console_script))
        .route("/console/assets/iconfont.woff2", get(console_iconfont))
        .route("/login", get(login_page))
        .route("/login/", get(login_page))
        .route("/login/assets/styles.css", get(login_styles))
        .route("/login/assets/app.js", get(login_script))
        .route("/healthz", get(healthz))
        .route("/api/admin/auth/login", post(admin_login))
        .route("/api/admin/auth/me", get(admin_me))
        .route(
            "/api/admin/auth/change-password",
            post(change_admin_password),
        )
        .route("/api/admin/auth/logout", post(admin_logout))
        .route("/api/admin/meta", get(admin_meta))
        .route("/api/admin/nodes", post(create_node).get(list_nodes))
        .route(
            "/api/admin/nodes/{node_id}",
            get(get_node_detail).delete(delete_node),
        )
        .route(
            "/api/admin/nodes/{node_id}/operations",
            post(create_node_operation).get(list_node_operations),
        )
        .route(
            "/api/admin/operations/templates",
            post(create_operation_template).get(list_operation_templates),
        )
        .route(
            "/api/admin/operations/templates/{template_id}",
            put(update_operation_template).delete(delete_operation_template),
        )
        .route("/api/admin/sites", post(create_site).get(list_sites))
        .route(
            "/api/admin/sites/{site_id}",
            get(get_site).put(update_site).delete(delete_site),
        )
        .route("/api/admin/sites/{site_id}/bindings", post(bind_site_nodes))
        .route("/api/admin/sites/{site_id}/enable", post(enable_site))
        .route("/api/admin/sites/{site_id}/disable", post(disable_site))
        .route(
            "/api/admin/sites/{site_id}/renew-certificate",
            post(renew_site_certificate),
        )
        .route(
            "/api/admin/sites/{site_id}/switch-primary",
            post(switch_site_primary),
        )
        .route(
            "/api/admin/releases",
            post(create_release).get(list_releases),
        )
        .route(
            "/api/admin/dns/providers",
            post(create_dns_provider).get(list_dns_providers),
        )
        .route(
            "/api/admin/dns/providers/{provider_id}",
            put(update_dns_provider).delete(delete_dns_provider),
        )
        .route("/api/admin/dns/zones/sync", post(sync_dns_zones))
        .route("/api/admin/dns/zones", get(list_dns_zones))
        .route(
            "/api/admin/dns/zones/{zone_id}/enable",
            post(enable_dns_zone),
        )
        .route(
            "/api/admin/dns/zones/{zone_id}/disable",
            post(disable_dns_zone),
        )
        .route("/api/admin/dns/zones/{zone_id}", delete(delete_dns_zone))
        .route(
            "/api/admin/certificates/orders",
            post(create_certificate_order).get(list_certificate_orders),
        )
        .route(
            "/api/admin/certificates/orders/{order_id}/retry",
            post(retry_certificate_order),
        )
        .route(
            "/api/admin/certificates/orders/{order_id}/reset",
            post(reset_certificate_order),
        )
        .route("/api/node/register", post(register_node))
        .route("/api/node/auth/refresh", post(refresh_node_auth))
        .route("/api/node/heartbeat", post(node_heartbeat))
        .route("/api/node/config/releases/latest", get(latest_release))
        .route("/api/node/config/package/{version}", get(config_package))
        .route("/api/node/releases/{release_id}/ack", post(ack_release))
        .route("/api/node/operations/pending", get(next_node_operation))
        .route(
            "/api/node/operations/{operation_id}/result",
            post(report_node_operation_result),
        )
        .with_state(state)
}

async fn healthz() -> Json<ApiResponse<HealthPayload>> {
    Json(ApiResponse::ok(
        Uuid::new_v4(),
        HealthPayload {
            service: "hub-api",
            status: "ok",
        },
    ))
}

async fn console_entry(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    if let Some(session_token) = extract_cookie_value(&headers, ADMIN_SESSION_COOKIE) {
        if state
            .service
            .authorize_admin_session(&session_token)
            .await
            .is_ok()
        {
            return Ok(console_index().await.into_response());
        }
    }
    Ok(Redirect::to("/login").into_response())
}

async fn login_page(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    if let Some(session_token) = extract_cookie_value(&headers, ADMIN_SESSION_COOKIE) {
        if state
            .service
            .authorize_admin_session(&session_token)
            .await
            .is_ok()
        {
            return Ok(Redirect::to("/").into_response());
        }
    }
    Ok(login_index().await.into_response())
}

async fn admin_login(
    State(state): State<AppState>,
    Json(request): Json<AdminLoginRequest>,
) -> Result<Response, AppError> {
    let (response, session_token) = state.service.login_admin(request).await?;
    let expires_at = response.session.expires_at;
    Ok(json_with_session_cookie(
        response,
        &session_token,
        expires_at,
    )?)
}

async fn admin_me(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<AdminMeResponse> {
    let session_token = extract_admin_session_cookie(&headers)?;
    let response = state.service.current_admin(&session_token).await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn change_admin_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ChangeAdminPasswordRequest>,
) -> ApiResult<ChangeAdminPasswordResponse> {
    let session_token = extract_admin_session_cookie(&headers)?;
    let response = state
        .service
        .change_admin_password(&session_token, request)
        .await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn admin_logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    if let Some(session_token) = extract_cookie_value(&headers, ADMIN_SESSION_COOKIE) {
        state.service.logout_admin(&session_token).await?;
    }
    Ok(json_with_clear_session_cookie(
        json!({ "logged_out": true }),
    )?)
}

async fn admin_meta(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<AdminMetaPayload> {
    require_admin_session(&state, &headers).await?;
    Ok(Json(ApiResponse::ok(
        Uuid::new_v4(),
        AdminMetaPayload {
            service: "hub-api",
            bind: state.config.bind,
            etcd_prefix: state.config.etcd.prefix,
            components: vec![
                "node-management",
                "release-management",
                "dns-integration",
                "failover-orchestration",
            ],
        },
    )))
}

async fn create_node(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateNodeRequest>,
) -> ApiResult<CreateNodeResponse> {
    require_admin_session(&state, &headers).await?;
    let response = state.service.create_node(request).await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn list_nodes(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Vec<NodeOverview>> {
    require_admin_session(&state, &headers).await?;
    let nodes = state.service.list_nodes().await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), nodes)))
}

async fn get_node_detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(node_id): Path<Uuid>,
) -> ApiResult<NodeDetail> {
    require_admin_session(&state, &headers).await?;
    let response = state.service.node_detail(node_id).await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn delete_node(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(node_id): Path<Uuid>,
) -> ApiResult<DeleteNodeResponse> {
    require_admin_session(&state, &headers).await?;
    let response = state.service.delete_node(node_id).await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn create_node_operation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(node_id): Path<Uuid>,
    Json(request): Json<CreateNodeOperationRequest>,
) -> ApiResult<NodeOperationOverview> {
    let admin = require_admin_session(&state, &headers).await?;
    let response = state
        .service
        .create_node_operation(
            node_id,
            request,
            &format!("admin:{}", admin.session.username),
        )
        .await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn list_node_operations(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(node_id): Path<Uuid>,
) -> ApiResult<Vec<NodeOperationOverview>> {
    require_admin_session(&state, &headers).await?;
    let response = state.service.list_node_operations(node_id).await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn create_operation_template(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateOperationTemplateRequest>,
) -> ApiResult<OperationTemplateOverview> {
    require_admin_session(&state, &headers).await?;
    let response = state.service.create_operation_template(request).await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn update_operation_template(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(template_id): Path<Uuid>,
    Json(request): Json<CreateOperationTemplateRequest>,
) -> ApiResult<OperationTemplateOverview> {
    require_admin_session(&state, &headers).await?;
    let response = state
        .service
        .update_operation_template(template_id, request)
        .await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn delete_operation_template(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(template_id): Path<Uuid>,
) -> ApiResult<DeleteOperationTemplateResponse> {
    require_admin_session(&state, &headers).await?;
    let response = state.service.delete_operation_template(template_id).await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn list_operation_templates(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Vec<OperationTemplateOverview>> {
    require_admin_session(&state, &headers).await?;
    let response = state.service.list_operation_templates().await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn register_node(
    State(state): State<AppState>,
    Json(request): Json<NodeRegisterRequest>,
) -> ApiResult<NodeRegisterResponse> {
    let response = state.service.register_node(request).await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn refresh_node_auth(
    State(state): State<AppState>,
    Json(request): Json<NodeRefreshRequest>,
) -> ApiResult<NodeRefreshResponse> {
    let response = state.service.refresh_node_token(request).await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn node_heartbeat(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<NodeHeartbeatRequest>,
) -> ApiResult<NodeHeartbeatResponse> {
    let access_token = extract_bearer_token(&headers)?;
    state
        .service
        .authorize_node(request.node_id, access_token)
        .await?;
    let response = state.service.node_heartbeat(request).await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn create_site(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateSiteRequest>,
) -> ApiResult<CreateSiteResponse> {
    require_admin_session(&state, &headers).await?;
    let response = state.service.create_site(request).await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn list_sites(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Vec<SiteOverview>> {
    require_admin_session(&state, &headers).await?;
    let sites = state.service.list_sites().await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), sites)))
}

async fn get_site(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(site_id): Path<Uuid>,
) -> ApiResult<SiteDetail> {
    require_admin_session(&state, &headers).await?;
    let site = state.service.site_detail(site_id).await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), site)))
}

async fn update_site(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(site_id): Path<Uuid>,
    Json(request): Json<UpdateSiteRequest>,
) -> ApiResult<CreateSiteResponse> {
    require_admin_session(&state, &headers).await?;
    let response = state.service.update_site(site_id, request).await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn delete_site(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(site_id): Path<Uuid>,
) -> ApiResult<DeleteSiteResponse> {
    require_admin_session(&state, &headers).await?;
    let response = state.service.delete_site(site_id).await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn enable_site(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(site_id): Path<Uuid>,
) -> ApiResult<UpdateSiteStatusResponse> {
    require_admin_session(&state, &headers).await?;
    let response = state.service.update_site_status(site_id, true).await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn disable_site(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(site_id): Path<Uuid>,
) -> ApiResult<UpdateSiteStatusResponse> {
    require_admin_session(&state, &headers).await?;
    let response = state.service.update_site_status(site_id, false).await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn bind_site_nodes(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(site_id): Path<Uuid>,
    Json(request): Json<CreateSiteBindingsRequest>,
) -> ApiResult<CreateSiteBindingsResponse> {
    require_admin_session(&state, &headers).await?;
    let response = state.service.bind_site_nodes(site_id, request).await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn renew_site_certificate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(site_id): Path<Uuid>,
) -> ApiResult<RenewSiteCertificateResponse> {
    require_admin_session(&state, &headers).await?;
    let response = state.service.renew_site_certificate(site_id).await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn switch_site_primary(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(site_id): Path<Uuid>,
    Json(request): Json<SwitchSitePrimaryRequest>,
) -> ApiResult<SwitchSitePrimaryResponse> {
    require_admin_session(&state, &headers).await?;
    let response = state.service.switch_site_primary(site_id, request).await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn create_release(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateReleaseRequest>,
) -> ApiResult<CreateReleaseResponse> {
    require_admin_session(&state, &headers).await?;
    let response = state.service.create_release(request).await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn list_releases(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Vec<ReleaseOverview>> {
    require_admin_session(&state, &headers).await?;
    let response = state.service.list_releases().await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn create_dns_provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateDnsProviderRequest>,
) -> ApiResult<CreateDnsProviderResponse> {
    require_admin_session(&state, &headers).await?;
    let response = state.service.create_dns_provider(request).await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn update_dns_provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(provider_id): Path<Uuid>,
    Json(request): Json<CreateDnsProviderRequest>,
) -> ApiResult<CreateDnsProviderResponse> {
    require_admin_session(&state, &headers).await?;
    let response = state
        .service
        .update_dns_provider(provider_id, request)
        .await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn delete_dns_provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(provider_id): Path<Uuid>,
) -> ApiResult<DeleteDnsProviderResponse> {
    require_admin_session(&state, &headers).await?;
    let response = state.service.delete_dns_provider(provider_id).await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn list_dns_providers(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Vec<DnsProviderOverview>> {
    require_admin_session(&state, &headers).await?;
    let response = state.service.list_dns_providers().await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn sync_dns_zones(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<SyncDnsZonesRequest>,
) -> ApiResult<SyncDnsZonesResponse> {
    require_admin_session(&state, &headers).await?;
    let response = state.service.sync_dns_zones(request).await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn list_dns_zones(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Vec<DnsZoneOverview>> {
    require_admin_session(&state, &headers).await?;
    let response = state.service.list_dns_zones().await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn enable_dns_zone(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(zone_id): Path<Uuid>,
) -> ApiResult<UpdateDnsZoneStatusResponse> {
    require_admin_session(&state, &headers).await?;
    let response = state.service.update_dns_zone_status(zone_id, true).await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn disable_dns_zone(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(zone_id): Path<Uuid>,
) -> ApiResult<UpdateDnsZoneStatusResponse> {
    require_admin_session(&state, &headers).await?;
    let response = state.service.update_dns_zone_status(zone_id, false).await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn delete_dns_zone(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(zone_id): Path<Uuid>,
) -> ApiResult<DeleteDnsZoneResponse> {
    require_admin_session(&state, &headers).await?;
    let response = state.service.delete_dns_zone(zone_id).await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn create_certificate_order(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateCertificateOrderRequest>,
) -> ApiResult<CreateCertificateOrderResponse> {
    require_admin_session(&state, &headers).await?;
    let response = state.service.create_certificate_order(request).await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn list_certificate_orders(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Vec<CertificateOrderOverview>> {
    require_admin_session(&state, &headers).await?;
    let response = state.service.list_certificate_orders().await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn retry_certificate_order(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(order_id): Path<Uuid>,
) -> ApiResult<CertificateOrderOverview> {
    require_admin_session(&state, &headers).await?;
    let response = state.service.retry_certificate_order(order_id).await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn reset_certificate_order(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(order_id): Path<Uuid>,
) -> ApiResult<CertificateOrderOverview> {
    require_admin_session(&state, &headers).await?;
    let response = state.service.reset_certificate_order(order_id).await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn latest_release(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<NodeScopedQuery>,
) -> ApiResult<LatestReleaseResponse> {
    let access_token = extract_bearer_token(&headers)?;
    state
        .service
        .authorize_node(query.node_id, access_token)
        .await?;
    let response = state.service.latest_release_for_node(query.node_id).await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn config_package(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(version): Path<String>,
    Query(query): Query<NodeScopedQuery>,
) -> ApiResult<ConfigPackageResponse> {
    let access_token = extract_bearer_token(&headers)?;
    state
        .service
        .authorize_node(query.node_id, access_token)
        .await?;
    let response = state
        .service
        .config_package_for_node(query.node_id, &version)
        .await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn ack_release(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(release_id): Path<Uuid>,
    Json(request): Json<ReleaseAckRequest>,
) -> ApiResult<ReleaseAckResponse> {
    let access_token = extract_bearer_token(&headers)?;
    state
        .service
        .authorize_node(request.node_id, access_token)
        .await?;
    let response = state.service.ack_release(release_id, request).await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn next_node_operation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<NodeScopedQuery>,
) -> ApiResult<PendingOperationResponse> {
    let access_token = extract_bearer_token(&headers)?;
    state
        .service
        .authorize_node(query.node_id, access_token)
        .await?;
    let response = state
        .service
        .claim_next_node_operation(query.node_id)
        .await?
        .ok_or_else(|| AppError::not_found("no pending operations"))?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn report_node_operation_result(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(operation_id): Path<Uuid>,
    Json(request): Json<OperationResultRequest>,
) -> ApiResult<OperationResultResponse> {
    let access_token = extract_bearer_token(&headers)?;
    state
        .service
        .authorize_node(request.node_id, access_token)
        .await?;
    let response = state
        .service
        .report_node_operation_result(operation_id, request)
        .await?;
    Ok(Json(ApiResponse::ok(Uuid::new_v4(), response)))
}

async fn require_admin_session(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<AdminMeResponse, AppError> {
    let session_token = extract_admin_session_cookie(headers)?;
    state.service.current_admin(&session_token).await
}

fn extract_bearer_token(headers: &HeaderMap) -> Result<&str, AppError> {
    let value = headers
        .get(axum::http::header::AUTHORIZATION)
        .ok_or_else(|| AppError::unauthorized("missing Authorization header"))?;
    let value = value
        .to_str()
        .map_err(|_| AppError::unauthorized("invalid Authorization header"))?;
    let Some(token) = value.strip_prefix("Bearer ") else {
        return Err(AppError::unauthorized(
            "Authorization header must use Bearer token",
        ));
    };
    if token.trim().is_empty() {
        return Err(AppError::unauthorized("Bearer token must not be empty"));
    }
    Ok(token)
}

fn extract_admin_session_cookie(headers: &HeaderMap) -> Result<String, AppError> {
    extract_cookie_value(headers, ADMIN_SESSION_COOKIE)
        .ok_or_else(|| AppError::unauthorized("missing admin session"))
}

fn extract_cookie_value(headers: &HeaderMap, cookie_name: &str) -> Option<String> {
    headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|raw| {
            raw.split(';').find_map(|item| {
                let (name, value) = item.trim().split_once('=')?;
                if name == cookie_name {
                    Some(value.to_string())
                } else {
                    None
                }
            })
        })
}

fn build_session_cookie(value: &str, max_age_seconds: i64) -> Result<HeaderValue, AppError> {
    HeaderValue::from_str(&format!(
        "{ADMIN_SESSION_COOKIE}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age_seconds}"
    ))
    .map_err(|error| AppError::internal(error.to_string()))
}

fn clear_session_cookie() -> Result<HeaderValue, AppError> {
    HeaderValue::from_str("pingorahub_admin_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0")
        .map_err(|error| AppError::internal(error.to_string()))
}

fn json_with_session_cookie<T: Serialize>(
    payload: T,
    session_token: &str,
    expires_at: chrono::DateTime<chrono::Utc>,
) -> Result<Response, AppError> {
    let max_age_seconds = (expires_at - chrono::Utc::now()).num_seconds().max(0);
    Ok((
        StatusCode::OK,
        [(
            header::SET_COOKIE,
            build_session_cookie(session_token, max_age_seconds)?,
        )],
        Json(ApiResponse::ok(Uuid::new_v4(), payload)),
    )
        .into_response())
}

fn json_with_clear_session_cookie<T: Serialize>(payload: T) -> Result<Response, AppError> {
    Ok((
        StatusCode::OK,
        [(header::SET_COOKIE, clear_session_cookie()?)],
        Json(ApiResponse::ok(Uuid::new_v4(), payload)),
    )
        .into_response())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    info!("shutdown signal received");
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "pingorahub_hub_api=info,info".into()),
        )
        .try_init();
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use axum::http::{StatusCode, header};
    use redis::AsyncCommands;
    use serde_json::{Value, json};
    use sqlx::PgPool;
    use tower::ServiceExt;

    #[tokio::test]
    async fn admin_auth_required_for_console_and_api() {
        let app = build_test_app().await;

        let console = app
            .clone()
            .oneshot(empty_request("GET", "/"))
            .await
            .unwrap();
        assert_eq!(console.status(), StatusCode::SEE_OTHER);
        assert_eq!(console.headers().get(header::LOCATION).unwrap(), "/login");

        let unauthorized = app
            .clone()
            .oneshot(empty_request("GET", "/api/admin/nodes"))
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let cookie = admin_session_cookie(&app).await;
        let me = app
            .oneshot(cookie_empty_request("GET", "/api/admin/auth/me", &cookie))
            .await
            .unwrap();
        assert_eq!(me.status(), StatusCode::OK);
        let me_body = response_json(me).await;
        assert_eq!(
            me_body["data"]["session"]["username"].as_str().unwrap(),
            "admin"
        );
    }

    #[tokio::test]
    async fn admin_can_change_password_and_old_password_stops_working() {
        let app = build_test_app().await;
        let cookie = admin_session_cookie(&app).await;

        let change = app
            .clone()
            .oneshot(cookie_json_request(
                "POST",
                "/api/admin/auth/change-password",
                &cookie,
                json!({
                    "current_password": "Admin#2026!",
                    "new_password": "Admin#2026!Next"
                }),
            ))
            .await
            .unwrap();
        assert_eq!(change.status(), StatusCode::OK);
        let change_body = response_json(change).await;
        assert_eq!(change_body["data"]["changed"].as_bool().unwrap(), true);
        assert_eq!(change_body["data"]["username"].as_str().unwrap(), "admin");

        let current_session = app
            .clone()
            .oneshot(cookie_empty_request("GET", "/api/admin/auth/me", &cookie))
            .await
            .unwrap();
        assert_eq!(current_session.status(), StatusCode::OK);

        let old_login = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/api/admin/auth/login",
                json!({
                    "username": "admin",
                    "password": "Admin#2026!"
                }),
            ))
            .await
            .unwrap();
        assert_eq!(old_login.status(), StatusCode::UNAUTHORIZED);

        let new_login = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/api/admin/auth/login",
                json!({
                    "username": "admin",
                    "password": "Admin#2026!Next"
                }),
            ))
            .await
            .unwrap();
        assert_eq!(new_login.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn p0_flow_works_end_to_end_with_token_refresh() {
        let app = build_test_app().await;
        let cookie = admin_session_cookie(&app).await;

        let create_node = app
            .clone()
            .oneshot(cookie_json_request(
                "POST",
                "/api/admin/nodes",
                &cookie,
                json!({
                    "node_code": "cn-sh-01",
                    "name": "上海入口一号",
                    "region": "cn-east",
                    "idc": "sh-a",
                    "labels": { "role": "edge" }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(create_node.status(), StatusCode::OK);
        let create_node_body = response_json(create_node).await;
        let node_id = create_node_body["data"]["node_id"]
            .as_str()
            .unwrap()
            .to_string();
        let bootstrap_token = create_node_body["data"]["bootstrap_token"]
            .as_str()
            .unwrap()
            .to_string();

        let register = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/api/node/register",
                json!({
                    "node_code": "cn-sh-01",
                    "bootstrap_token": bootstrap_token,
                    "hostname": "edge-01",
                    "public_ip": "1.1.1.1",
                    "private_ip": "10.0.0.10",
                    "agent_version": "0.1.0"
                }),
            ))
            .await
            .unwrap();
        assert_eq!(register.status(), StatusCode::OK);
        let register_body = response_json(register).await;
        let refresh_token = register_body["data"]["refresh_token"]
            .as_str()
            .unwrap()
            .to_string();

        let refresh = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/api/node/auth/refresh",
                json!({
                    "node_id": node_id,
                    "refresh_token": refresh_token
                }),
            ))
            .await
            .unwrap();
        assert_eq!(refresh.status(), StatusCode::OK);
        let refresh_body = response_json(refresh).await;
        let access_token = refresh_body["data"]["access_token"]
            .as_str()
            .unwrap()
            .to_string();

        let heartbeat = app
            .clone()
            .oneshot(auth_json_request(
                "POST",
                "/api/node/heartbeat",
                &access_token,
                json!({
                    "node_id": node_id,
                    "pingora_version": "0.5.0",
                    "agent_version": "0.1.0",
                    "active_config_version": null,
                    "site_count": 0,
                    "cpu_usage": 1.0,
                    "mem_usage": 2.0,
                    "disk_usage": 3.0,
                    "health_score": 99
                }),
            ))
            .await
            .unwrap();
        assert_eq!(heartbeat.status(), StatusCode::OK);

        let create_site = app
            .clone()
            .oneshot(cookie_json_request(
                "POST",
                "/api/admin/sites",
                &cookie,
                json!({
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
                                "endpoints": ["10.0.1.10:8443", "10.0.1.11:8443"]
                            }
                        ],
                        "cache_rules": [
                            {
                                "name": "images",
                                "match_extensions": ["jpg", "jpeg", "png", "webp"],
                                "expires_seconds": 2592000,
                                "cache_control": "public, max-age=2592000, immutable"
                            },
                            {
                                "name": "css-js",
                                "match_extensions": ["css", "js"],
                                "expires_seconds": 604800,
                                "cache_control": "public, max-age=604800, immutable"
                            }
                        ]
                    }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(create_site.status(), StatusCode::OK);
        let site_body = response_json(create_site).await;
        let site_id = site_body["data"]["site_id"].as_str().unwrap().to_string();

        let bind = app
            .clone()
            .oneshot(cookie_json_request(
                "POST",
                &format!("/api/admin/sites/{site_id}/bindings"),
                &cookie,
                json!({
                    "bindings": [
                        {
                            "node_id": node_id,
                            "binding_role": "primary",
                            "priority": 10
                        }
                    ]
                }),
            ))
            .await
            .unwrap();
        assert_eq!(bind.status(), StatusCode::OK);

        let update_site = app
            .clone()
            .oneshot(cookie_json_request(
                "PUT",
                &format!("/api/admin/sites/{site_id}"),
                &cookie,
                json!({
                    "name": "portal-prod-v2",
                    "domain": "portal.example.com",
                    "listen_port": 443,
                    "protocol": "https",
                    "tls_enabled": true,
                    "config": {
                        "upstreams": [
                            {
                                "name": "portal-origin",
                                "balance_method": "least_connections",
                                "endpoints": [
                                    {"address": "10.0.2.10:8443", "weight": 80, "active": true},
                                    {"address": "10.0.2.11:8443", "weight": 20, "active": true}
                                ]
                            }
                        ],
                        "cache_rules": [
                            {
                                "name": "images",
                                "match_extensions": ["jpg", "jpeg", "png", "svg", "webp"],
                                "expires_seconds": 2592000,
                                "cache_control": "public, max-age=2592000, immutable"
                            },
                            {
                                "name": "css-js",
                                "match_extensions": ["css", "js", "mjs", "map"],
                                "expires_seconds": 604800,
                                "cache_control": "public, max-age=604800, immutable"
                            }
                        ]
                    }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(update_site.status(), StatusCode::OK);
        let update_site_body = response_json(update_site).await;
        assert_eq!(update_site_body["data"]["version"].as_u64().unwrap(), 2);

        let release = app
            .clone()
            .oneshot(cookie_json_request(
                "POST",
                "/api/admin/releases",
                &cookie,
                json!({
                    "scope_type": "site",
                    "scope_id": site_id,
                    "release_type": "publish",
                    "reason": "initial publish"
                }),
            ))
            .await
            .unwrap();
        assert_eq!(release.status(), StatusCode::OK);
        let release_body = response_json(release).await;
        let release_id = release_body["data"]["release_id"]
            .as_str()
            .unwrap()
            .to_string();
        let release_version = release_body["data"]["release_version"]
            .as_str()
            .unwrap()
            .to_string();

        let latest = app
            .clone()
            .oneshot(auth_empty_request(
                "GET",
                &format!("/api/node/config/releases/latest?node_id={node_id}"),
                &access_token,
            ))
            .await
            .unwrap();
        assert_eq!(latest.status(), StatusCode::OK);

        let package = app
            .clone()
            .oneshot(auth_empty_request(
                "GET",
                &format!("/api/node/config/package/{release_version}?node_id={node_id}"),
                &access_token,
            ))
            .await
            .unwrap();
        assert_eq!(package.status(), StatusCode::OK);
        let package_body = response_json(package).await;
        assert_eq!(
            package_body["data"]["release_version"].as_str().unwrap(),
            release_version
        );
        assert_eq!(
            package_body["data"]["rendered_config"]["cache_rules"][0]["name"]
                .as_str()
                .unwrap(),
            "images"
        );
        assert_eq!(
            package_body["data"]["rendered_config"]["cache_rules"][1]["name"]
                .as_str()
                .unwrap(),
            "css-js"
        );
        assert_eq!(
            package_body["data"]["rendered_config"]["upstreams"][0]["balance_method"]
                .as_str()
                .unwrap(),
            "least_connections"
        );
        assert_eq!(
            package_body["data"]["rendered_config"]["upstreams"][0]["endpoints"][0]["address"]
                .as_str()
                .unwrap(),
            "10.0.2.10:8443"
        );
        assert_eq!(
            package_body["data"]["rendered_config"]["upstreams"][0]["endpoints"][1]["address"]
                .as_str()
                .unwrap(),
            "10.0.2.11:8443"
        );
        assert_eq!(
            package_body["data"]["rendered_config"]["routes"][0]["path"]
                .as_str()
                .unwrap(),
            "/"
        );
        assert_eq!(
            package_body["data"]["rendered_config"]["routes"][0]["upstream"]
                .as_str()
                .unwrap(),
            "portal-origin"
        );
        assert_eq!(
            package_body["data"]["manifest"]["sites"][0]["cache_rules"][0]["expires_seconds"]
                .as_u64()
                .unwrap(),
            2592000
        );

        let ack = app
            .clone()
            .oneshot(auth_json_request(
                "POST",
                &format!("/api/node/releases/{release_id}/ack"),
                &access_token,
                json!({
                    "node_id": node_id,
                    "apply_status": "success",
                    "current_version": release_version,
                    "message": "reloaded successfully"
                }),
            ))
            .await
            .unwrap();
        assert_eq!(ack.status(), StatusCode::OK);

        let releases = app
            .clone()
            .oneshot(cookie_empty_request("GET", "/api/admin/releases", &cookie))
            .await
            .unwrap();
        assert_eq!(releases.status(), StatusCode::OK);
        let releases_body = response_json(releases).await;
        assert_eq!(
            releases_body["data"][0]["status"].as_str().unwrap(),
            "success"
        );
        assert_eq!(
            releases_body["data"][0]["counts"]["success"]
                .as_u64()
                .unwrap(),
            1
        );

        let nodes = app
            .oneshot(cookie_empty_request("GET", "/api/admin/nodes", &cookie))
            .await
            .unwrap();
        assert_eq!(nodes.status(), StatusCode::OK);
        let nodes_body = response_json(nodes).await;
        assert_eq!(
            nodes_body["data"][0]["active_config_version"]
                .as_str()
                .unwrap(),
            release_version
        );
    }

    #[tokio::test]
    async fn site_routes_are_published_in_config_package() {
        let app = build_test_app().await;
        let cookie = admin_session_cookie(&app).await;

        let create_node = app
            .clone()
            .oneshot(cookie_json_request(
                "POST",
                "/api/admin/nodes",
                &cookie,
                json!({
                    "node_code": "route-node",
                    "name": "Route Node",
                    "region": "cn-east",
                    "idc": "lab",
                    "labels": { "role": "edge" }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(create_node.status(), StatusCode::OK);
        let create_node_body = response_json(create_node).await;
        let node_id = create_node_body["data"]["node_id"]
            .as_str()
            .unwrap()
            .to_string();
        let bootstrap_token = create_node_body["data"]["bootstrap_token"]
            .as_str()
            .unwrap()
            .to_string();

        let register = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/api/node/register",
                json!({
                    "node_code": "route-node",
                    "bootstrap_token": bootstrap_token,
                    "hostname": "route-node.local",
                    "private_ip": "10.0.3.10",
                    "public_ip": "203.0.113.10",
                    "agent_version": "0.1.0"
                }),
            ))
            .await
            .unwrap();
        assert_eq!(register.status(), StatusCode::OK);
        let register_body = response_json(register).await;
        let access_token = register_body["data"]["access_token"]
            .as_str()
            .unwrap()
            .to_string();

        let create_site = app
            .clone()
            .oneshot(cookie_json_request(
                "POST",
                "/api/admin/sites",
                &cookie,
                json!({
                    "site_code": "route-demo",
                    "name": "route-demo",
                    "domain": "route.example.com",
                    "listen_port": 80,
                    "protocol": "http",
                    "tls_enabled": false,
                    "config": {
                        "upstreams": [
                            {
                                "name": "web",
                                "endpoints": ["10.0.3.10:8080"]
                            },
                            {
                                "name": "api",
                                "balance_method": "weighted_round_robin",
                                "endpoints": [
                                    {"address": "10.0.3.11:9000", "weight": 100, "active": true}
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
                }),
            ))
            .await
            .unwrap();
        assert_eq!(create_site.status(), StatusCode::OK);
        let create_site_body = response_json(create_site).await;
        let site_id = create_site_body["data"]["site_id"]
            .as_str()
            .unwrap()
            .to_string();

        let bind = app
            .clone()
            .oneshot(cookie_json_request(
                "POST",
                &format!("/api/admin/sites/{site_id}/bindings"),
                &cookie,
                json!({
                    "bindings": [
                        {
                            "node_id": node_id,
                            "binding_role": "primary",
                            "priority": 10
                        }
                    ]
                }),
            ))
            .await
            .unwrap();
        assert_eq!(bind.status(), StatusCode::OK);

        let release = app
            .clone()
            .oneshot(cookie_json_request(
                "POST",
                "/api/admin/releases",
                &cookie,
                json!({
                    "scope_type": "site",
                    "scope_id": site_id,
                    "release_type": "publish",
                    "reason": "publish route demo"
                }),
            ))
            .await
            .unwrap();
        assert_eq!(release.status(), StatusCode::OK);
        let release_body = response_json(release).await;
        let release_version = release_body["data"]["release_version"]
            .as_str()
            .unwrap()
            .to_string();

        let package = app
            .clone()
            .oneshot(auth_empty_request(
                "GET",
                &format!("/api/node/config/package/{release_version}?node_id={node_id}"),
                &access_token,
            ))
            .await
            .unwrap();
        assert_eq!(package.status(), StatusCode::OK);
        let package_body = response_json(package).await;
        assert_eq!(
            package_body["data"]["rendered_config"]["routes"][0]["upstream"],
            "api"
        );
        assert_eq!(
            package_body["data"]["rendered_config"]["routes"][1]["path"],
            "/"
        );
    }

    #[tokio::test]
    async fn create_site_rejects_route_with_unknown_upstream() {
        let app = build_test_app().await;
        let cookie = admin_session_cookie(&app).await;

        let create_site = app
            .clone()
            .oneshot(cookie_json_request(
                "POST",
                "/api/admin/sites",
                &cookie,
                json!({
                    "site_code": "route-invalid",
                    "name": "route-invalid",
                    "domain": "route-invalid.example.com",
                    "listen_port": 80,
                    "protocol": "http",
                    "tls_enabled": false,
                    "config": {
                        "upstreams": [
                            {
                                "name": "web",
                                "endpoints": ["10.0.3.10:8080"]
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
                }),
            ))
            .await
            .unwrap();

        assert_eq!(create_site.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn delete_node_removes_it_and_revokes_tokens() {
        let app = build_test_app().await;
        let cookie = admin_session_cookie(&app).await;

        let create_node = app
            .clone()
            .oneshot(cookie_json_request(
                "POST",
                "/api/admin/nodes",
                &cookie,
                json!({
                    "node_code": "delete-me",
                    "name": "Delete Me",
                    "region": "cn-east",
                    "idc": "lab",
                    "labels": { "role": "edge" }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(create_node.status(), StatusCode::OK);
        let create_node_body = response_json(create_node).await;
        let node_id = create_node_body["data"]["node_id"]
            .as_str()
            .unwrap()
            .to_string();
        let bootstrap_token = create_node_body["data"]["bootstrap_token"]
            .as_str()
            .unwrap()
            .to_string();

        let register = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/api/node/register",
                json!({
                    "node_code": "delete-me",
                    "bootstrap_token": bootstrap_token,
                    "hostname": "delete-me-host",
                    "public_ip": "1.1.1.2",
                    "private_ip": "10.0.0.20",
                    "agent_version": "0.1.0"
                }),
            ))
            .await
            .unwrap();
        assert_eq!(register.status(), StatusCode::OK);
        let register_body = response_json(register).await;
        let access_token = register_body["data"]["access_token"]
            .as_str()
            .unwrap()
            .to_string();

        let delete_response = app
            .clone()
            .oneshot(cookie_empty_request(
                "DELETE",
                &format!("/api/admin/nodes/{node_id}"),
                &cookie,
            ))
            .await
            .unwrap();
        assert_eq!(delete_response.status(), StatusCode::OK);
        let delete_body = response_json(delete_response).await;
        assert_eq!(delete_body["data"]["deleted"].as_bool().unwrap(), true);
        assert_eq!(
            delete_body["data"]["node_code"].as_str().unwrap(),
            "delete-me"
        );

        let nodes = app
            .clone()
            .oneshot(cookie_empty_request("GET", "/api/admin/nodes", &cookie))
            .await
            .unwrap();
        assert_eq!(nodes.status(), StatusCode::OK);
        let nodes_body = response_json(nodes).await;
        assert!(
            nodes_body["data"]
                .as_array()
                .unwrap()
                .iter()
                .all(|node| node["node_code"].as_str().unwrap() != "delete-me")
        );

        let heartbeat = app
            .oneshot(auth_json_request(
                "POST",
                "/api/node/heartbeat",
                &access_token,
                json!({
                    "node_id": node_id,
                    "pingora_version": "0.5.0",
                    "agent_version": "0.1.0",
                    "active_config_version": null,
                    "site_count": 0,
                    "cpu_usage": 1.0,
                    "mem_usage": 2.0,
                    "disk_usage": 3.0,
                    "health_score": 99
                }),
            ))
            .await
            .unwrap();
        assert_eq!(heartbeat.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn delete_node_rejects_bound_sites() {
        let app = build_test_app().await;
        let cookie = admin_session_cookie(&app).await;

        let create_node = app
            .clone()
            .oneshot(cookie_json_request(
                "POST",
                "/api/admin/nodes",
                &cookie,
                json!({
                    "node_code": "bound-node",
                    "name": "Bound Node",
                    "region": "cn-east",
                    "idc": "lab",
                    "labels": { "role": "edge" }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(create_node.status(), StatusCode::OK);
        let create_node_body = response_json(create_node).await;
        let node_id = create_node_body["data"]["node_id"]
            .as_str()
            .unwrap()
            .to_string();

        let create_site = app
            .clone()
            .oneshot(cookie_json_request(
                "POST",
                "/api/admin/sites",
                &cookie,
                json!({
                    "site_code": "bound-site",
                    "name": "bound-site",
                    "domain": "bound.example.com",
                    "listen_port": 443,
                    "protocol": "https",
                    "tls_enabled": true,
                    "config": { "upstreams": [] }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(create_site.status(), StatusCode::OK);
        let create_site_body = response_json(create_site).await;
        let site_id = create_site_body["data"]["site_id"]
            .as_str()
            .unwrap()
            .to_string();

        let bind = app
            .clone()
            .oneshot(cookie_json_request(
                "POST",
                &format!("/api/admin/sites/{site_id}/bindings"),
                &cookie,
                json!({
                    "bindings": [
                        {
                            "node_id": node_id,
                            "binding_role": "primary",
                            "priority": 10
                        }
                    ]
                }),
            ))
            .await
            .unwrap();
        assert_eq!(bind.status(), StatusCode::OK);

        let delete_response = app
            .oneshot(cookie_empty_request(
                "DELETE",
                &format!("/api/admin/nodes/{node_id}"),
                &cookie,
            ))
            .await
            .unwrap();
        assert_eq!(delete_response.status(), StatusCode::CONFLICT);
        let delete_body = response_json(delete_response).await;
        assert!(
            delete_body["error"]["message"]
                .as_str()
                .unwrap()
                .contains("bound-site")
        );
    }

    #[tokio::test]
    async fn node_detail_returns_runtime_and_bound_sites() {
        let app = build_test_app().await;
        let cookie = admin_session_cookie(&app).await;

        let create_node = app
            .clone()
            .oneshot(cookie_json_request(
                "POST",
                "/api/admin/nodes",
                &cookie,
                json!({
                    "node_code": "detail-node",
                    "name": "Detail Node",
                    "region": "cn-east",
                    "idc": "lab",
                    "labels": { "role": "edge", "host": "detail-host" }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(create_node.status(), StatusCode::OK);
        let create_node_body = response_json(create_node).await;
        let node_id = create_node_body["data"]["node_id"]
            .as_str()
            .unwrap()
            .to_string();
        let bootstrap_token = create_node_body["data"]["bootstrap_token"]
            .as_str()
            .unwrap()
            .to_string();

        let register = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/api/node/register",
                json!({
                    "node_code": "detail-node",
                    "bootstrap_token": bootstrap_token,
                    "hostname": "detail-host",
                    "public_ip": "203.0.113.10",
                    "private_ip": "10.10.0.10",
                    "agent_version": "0.2.0"
                }),
            ))
            .await
            .unwrap();
        assert_eq!(register.status(), StatusCode::OK);
        let register_body = response_json(register).await;
        let access_token = register_body["data"]["access_token"]
            .as_str()
            .unwrap()
            .to_string();

        let heartbeat = app
            .clone()
            .oneshot(auth_json_request(
                "POST",
                "/api/node/heartbeat",
                &access_token,
                json!({
                    "node_id": node_id,
                    "pingora_version": "0.5.0",
                    "agent_version": "0.2.0",
                    "active_config_version": "rel-node-detail-001",
                    "site_count": 2,
                    "cpu_usage": 10.0,
                    "mem_usage": 30.0,
                    "disk_usage": 40.0,
                    "health_score": 97
                }),
            ))
            .await
            .unwrap();
        assert_eq!(heartbeat.status(), StatusCode::OK);

        let create_site = app
            .clone()
            .oneshot(cookie_json_request(
                "POST",
                "/api/admin/sites",
                &cookie,
                json!({
                    "site_code": "detail-site",
                    "name": "Detail Site",
                    "domain": "detail.example.com",
                    "listen_port": 443,
                    "protocol": "https",
                    "tls_enabled": true,
                    "config": { "upstreams": [] }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(create_site.status(), StatusCode::OK);
        let create_site_body = response_json(create_site).await;
        let site_id = create_site_body["data"]["site_id"]
            .as_str()
            .unwrap()
            .to_string();

        let bind = app
            .clone()
            .oneshot(cookie_json_request(
                "POST",
                &format!("/api/admin/sites/{site_id}/bindings"),
                &cookie,
                json!({
                    "bindings": [
                        {
                            "node_id": node_id,
                            "binding_role": "primary",
                            "priority": 10
                        }
                    ]
                }),
            ))
            .await
            .unwrap();
        assert_eq!(bind.status(), StatusCode::OK);

        let detail = app
            .oneshot(cookie_empty_request(
                "GET",
                &format!("/api/admin/nodes/{node_id}"),
                &cookie,
            ))
            .await
            .unwrap();
        assert_eq!(detail.status(), StatusCode::OK);
        let detail_body = response_json(detail).await;
        assert_eq!(
            detail_body["data"]["active_config_version"]
                .as_str()
                .unwrap(),
            "rel-node-detail-001"
        );
        assert_eq!(
            detail_body["data"]["hostname"].as_str().unwrap(),
            "detail-host"
        );
        assert_eq!(
            detail_body["data"]["runtime_site_count"].as_u64().unwrap(),
            2
        );
        assert_eq!(detail_body["data"]["health_score"].as_u64().unwrap(), 97);
        assert_eq!(detail_body["data"]["sites"].as_array().unwrap().len(), 1);
        assert_eq!(
            detail_body["data"]["sites"][0]["site_code"]
                .as_str()
                .unwrap(),
            "detail-site"
        );
        assert_eq!(
            detail_body["data"]["sites"][0]["binding_role"]
                .as_str()
                .unwrap(),
            "primary"
        );
    }

    #[tokio::test]
    async fn node_endpoint_requires_bearer_token() {
        let app = build_test_app().await;

        let response = app
            .oneshot(empty_request(
                "GET",
                &format!(
                    "/api/node/config/releases/latest?node_id={}",
                    Uuid::new_v4()
                ),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn site_detail_requires_login_and_works_after_login() {
        let app = build_test_app().await;
        let cookie = admin_session_cookie(&app).await;

        let create_site = app
            .clone()
            .oneshot(cookie_json_request(
                "POST",
                "/api/admin/sites",
                &cookie,
                json!({
                    "site_code": "console-demo",
                    "name": "console-demo",
                    "domain": "console.example.com",
                    "listen_port": 443,
                    "protocol": "https",
                    "tls_enabled": true,
                    "config": { "upstreams": [] }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(create_site.status(), StatusCode::OK);
        let site_body = response_json(create_site).await;
        let site_id = site_body["data"]["site_id"].as_str().unwrap();

        let detail = app
            .oneshot(cookie_empty_request(
                "GET",
                &format!("/api/admin/sites/{site_id}"),
                &cookie,
            ))
            .await
            .unwrap();
        assert_eq!(detail.status(), StatusCode::OK);
        let detail_body = response_json(detail).await;
        assert_eq!(
            detail_body["data"]["site_code"].as_str().unwrap(),
            "console-demo"
        );
        assert_eq!(detail_body["data"]["protocol"].as_str().unwrap(), "https");
    }

    #[tokio::test]
    async fn dns_and_certificate_order_flow_works() {
        let app = build_test_app().await;
        let cookie = admin_session_cookie(&app).await;

        let create_site = app
            .clone()
            .oneshot(cookie_json_request(
                "POST",
                "/api/admin/sites",
                &cookie,
                json!({
                    "site_code": "cert-demo",
                    "name": "cert-demo",
                    "domain": "demo.example.com",
                    "listen_port": 443,
                    "protocol": "https",
                    "tls_enabled": true,
                    "config": { "upstreams": [] }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(create_site.status(), StatusCode::OK);
        let site_body = response_json(create_site).await;
        let site_id = site_body["data"]["site_id"].as_str().unwrap().to_string();

        let provider = app
            .clone()
            .oneshot(cookie_json_request(
                "POST",
                "/api/admin/dns/providers",
                &cookie,
                json!({
                    "name": "cf-prod",
                    "provider_type": "noop",
                    "credentials": {}
                }),
            ))
            .await
            .unwrap();
        assert_eq!(provider.status(), StatusCode::OK);
        let provider_body = response_json(provider).await;
        let provider_id = provider_body["data"]["provider_id"]
            .as_str()
            .unwrap()
            .to_string();

        let zone_sync = app
            .clone()
            .oneshot(cookie_json_request(
                "POST",
                "/api/admin/dns/zones/sync",
                &cookie,
                json!({
                    "provider_id": provider_id,
                    "zone_names": ["example.com"]
                }),
            ))
            .await
            .unwrap();
        assert_eq!(zone_sync.status(), StatusCode::OK);
        let zone_sync_body = response_json(zone_sync).await;
        let zone_id = zone_sync_body["data"]["zone_ids"][0]
            .as_str()
            .unwrap()
            .to_string();

        let zone_sync_again = app
            .clone()
            .oneshot(cookie_json_request(
                "POST",
                "/api/admin/dns/zones/sync",
                &cookie,
                json!({
                    "provider_id": provider_id,
                    "zone_names": ["example.com"]
                }),
            ))
            .await
            .unwrap();
        assert_eq!(zone_sync_again.status(), StatusCode::OK);

        let order = app
            .clone()
            .oneshot(cookie_json_request(
                "POST",
                "/api/admin/certificates/orders",
                &cookie,
                json!({
                    "site_id": site_id,
                    "acme_provider": "letsencrypt",
                    "challenge_type": "dns-01",
                    "zone_id": zone_id
                }),
            ))
            .await
            .unwrap();
        assert_eq!(order.status(), StatusCode::OK);
        let order_body = response_json(order).await;
        assert_eq!(
            order_body["data"]["order_status"].as_str().unwrap(),
            "pending_dns_challenge"
        );
        assert!(
            order_body["data"]["challenge_payload"]["record_name"]
                .as_str()
                .unwrap()
                .contains("_acme-challenge")
        );

        let providers = app
            .clone()
            .oneshot(cookie_empty_request(
                "GET",
                "/api/admin/dns/providers",
                &cookie,
            ))
            .await
            .unwrap();
        assert_eq!(providers.status(), StatusCode::OK);

        let zones = app
            .clone()
            .oneshot(cookie_empty_request("GET", "/api/admin/dns/zones", &cookie))
            .await
            .unwrap();
        assert_eq!(zones.status(), StatusCode::OK);
        let zones_body = response_json(zones).await;
        assert_eq!(zones_body["data"].as_array().unwrap().len(), 1);

        let orders = app
            .oneshot(cookie_empty_request(
                "GET",
                "/api/admin/certificates/orders",
                &cookie,
            ))
            .await
            .unwrap();
        assert_eq!(orders.status(), StatusCode::OK);
        let orders_body = response_json(orders).await;
        assert_eq!(
            orders_body["data"][0]["challenge_type"].as_str().unwrap(),
            "dns-01"
        );
    }

    #[tokio::test]
    async fn renew_site_certificate_auto_selects_zone() {
        let app = build_test_app().await;
        let cookie = admin_session_cookie(&app).await;

        let create_site = app
            .clone()
            .oneshot(cookie_json_request(
                "POST",
                "/api/admin/sites",
                &cookie,
                json!({
                    "site_code": "renew-demo",
                    "name": "renew-demo",
                    "domain": "renew.example.com",
                    "listen_port": 443,
                    "protocol": "https",
                    "tls_enabled": true,
                    "config": { "upstreams": [] }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(create_site.status(), StatusCode::OK);
        let site_body = response_json(create_site).await;
        let site_id = site_body["data"]["site_id"].as_str().unwrap().to_string();

        let provider = app
            .clone()
            .oneshot(cookie_json_request(
                "POST",
                "/api/admin/dns/providers",
                &cookie,
                json!({
                    "name": "renew-cf",
                    "provider_type": "noop",
                    "credentials": {}
                }),
            ))
            .await
            .unwrap();
        assert_eq!(provider.status(), StatusCode::OK);
        let provider_body = response_json(provider).await;
        let provider_id = provider_body["data"]["provider_id"]
            .as_str()
            .unwrap()
            .to_string();

        let zone_sync = app
            .clone()
            .oneshot(cookie_json_request(
                "POST",
                "/api/admin/dns/zones/sync",
                &cookie,
                json!({
                    "provider_id": provider_id,
                    "zone_names": ["example.com"]
                }),
            ))
            .await
            .unwrap();
        assert_eq!(zone_sync.status(), StatusCode::OK);
        let zone_body = response_json(zone_sync).await;
        let zone_id = zone_body["data"]["zone_ids"][0]
            .as_str()
            .unwrap()
            .to_string();

        let renew = app
            .clone()
            .oneshot(cookie_empty_request(
                "POST",
                &format!("/api/admin/sites/{site_id}/renew-certificate"),
                &cookie,
            ))
            .await
            .unwrap();
        assert_eq!(renew.status(), StatusCode::OK);
        let renew_body = response_json(renew).await;
        assert_eq!(
            renew_body["data"]["domain"].as_str().unwrap(),
            "renew.example.com"
        );
        assert_eq!(
            renew_body["data"]["zone_id"].as_str().unwrap(),
            zone_id.as_str()
        );
        assert_eq!(
            renew_body["data"]["order_status"].as_str().unwrap(),
            "pending_dns_challenge"
        );
    }

    #[tokio::test]
    async fn switch_site_primary_promotes_standby_and_creates_release() {
        let app = build_test_app().await;
        let cookie = admin_session_cookie(&app).await;

        let primary_node = app
            .clone()
            .oneshot(cookie_json_request(
                "POST",
                "/api/admin/nodes",
                &cookie,
                json!({
                    "node_code": "switch-primary",
                    "name": "Primary",
                    "region": "cn-east",
                    "idc": "lab",
                    "labels": { "role": "edge" }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(primary_node.status(), StatusCode::OK);
        let primary_node_body = response_json(primary_node).await;
        let primary_node_id = primary_node_body["data"]["node_id"]
            .as_str()
            .unwrap()
            .to_string();

        let standby_node = app
            .clone()
            .oneshot(cookie_json_request(
                "POST",
                "/api/admin/nodes",
                &cookie,
                json!({
                    "node_code": "switch-standby",
                    "name": "Standby",
                    "region": "cn-east",
                    "idc": "lab",
                    "labels": { "role": "edge" }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(standby_node.status(), StatusCode::OK);
        let standby_node_body = response_json(standby_node).await;
        let standby_node_id = standby_node_body["data"]["node_id"]
            .as_str()
            .unwrap()
            .to_string();

        let create_site = app
            .clone()
            .oneshot(cookie_json_request(
                "POST",
                "/api/admin/sites",
                &cookie,
                json!({
                    "site_code": "switch-demo",
                    "name": "switch-demo",
                    "domain": "switch.example.com",
                    "listen_port": 443,
                    "protocol": "https",
                    "tls_enabled": true,
                    "config": { "upstreams": [] }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(create_site.status(), StatusCode::OK);
        let site_body = response_json(create_site).await;
        let site_id = site_body["data"]["site_id"].as_str().unwrap().to_string();

        let bind = app
            .clone()
            .oneshot(cookie_json_request(
                "POST",
                &format!("/api/admin/sites/{site_id}/bindings"),
                &cookie,
                json!({
                    "bindings": [
                        {
                            "node_id": primary_node_id,
                            "binding_role": "primary",
                            "priority": 100
                        },
                        {
                            "node_id": standby_node_id,
                            "binding_role": "standby",
                            "priority": 200
                        }
                    ]
                }),
            ))
            .await
            .unwrap();
        assert_eq!(bind.status(), StatusCode::OK);

        let switch = app
            .clone()
            .oneshot(cookie_json_request(
                "POST",
                &format!("/api/admin/sites/{site_id}/switch-primary"),
                &cookie,
                json!({}),
            ))
            .await
            .unwrap();
        assert_eq!(switch.status(), StatusCode::OK);
        let switch_body = response_json(switch).await;
        assert_eq!(
            switch_body["data"]["previous_primary_node_id"]
                .as_str()
                .unwrap(),
            primary_node_id.as_str()
        );
        assert_eq!(
            switch_body["data"]["current_primary_node_id"]
                .as_str()
                .unwrap(),
            standby_node_id.as_str()
        );

        let detail = app
            .clone()
            .oneshot(cookie_empty_request(
                "GET",
                &format!("/api/admin/sites/{site_id}"),
                &cookie,
            ))
            .await
            .unwrap();
        assert_eq!(detail.status(), StatusCode::OK);
        let detail_body = response_json(detail).await;
        assert_eq!(
            detail_body["data"]["bindings"][0]["node_id"]
                .as_str()
                .unwrap(),
            standby_node_id.as_str()
        );
        assert_eq!(
            detail_body["data"]["bindings"][0]["binding_role"]
                .as_str()
                .unwrap(),
            "primary"
        );
        assert_eq!(
            detail_body["data"]["bindings"][1]["node_id"]
                .as_str()
                .unwrap(),
            primary_node_id.as_str()
        );
        assert_eq!(
            detail_body["data"]["bindings"][1]["binding_role"]
                .as_str()
                .unwrap(),
            "standby"
        );

        let releases = app
            .oneshot(cookie_empty_request("GET", "/api/admin/releases", &cookie))
            .await
            .unwrap();
        assert_eq!(releases.status(), StatusCode::OK);
        let releases_body = response_json(releases).await;
        assert_eq!(
            releases_body["data"][0]["release_type"].as_str().unwrap(),
            "switch"
        );
    }

    #[tokio::test]
    async fn switch_site_primary_accepts_target_and_reports_next_standby() {
        let app = build_test_app().await;
        let cookie = admin_session_cookie(&app).await;

        let mut node_ids = Vec::new();
        for node_code in [
            "switch-target-primary",
            "switch-target-a",
            "switch-target-b",
        ] {
            let response = app
                .clone()
                .oneshot(cookie_json_request(
                    "POST",
                    "/api/admin/nodes",
                    &cookie,
                    json!({
                        "node_code": node_code,
                        "name": node_code,
                        "region": "cn-east",
                        "idc": "lab",
                        "labels": { "role": "edge" }
                    }),
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            node_ids.push(
                response_json(response).await["data"]["node_id"]
                    .as_str()
                    .unwrap()
                    .to_string(),
            );
        }

        let create_site = app
            .clone()
            .oneshot(cookie_json_request(
                "POST",
                "/api/admin/sites",
                &cookie,
                json!({
                    "site_code": "switch-target-demo",
                    "name": "switch-target-demo",
                    "domain": "switch-target.example.com",
                    "listen_port": 443,
                    "protocol": "https",
                    "tls_enabled": true,
                    "config": { "upstreams": [] }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(create_site.status(), StatusCode::OK);
        let site_id = response_json(create_site).await["data"]["site_id"]
            .as_str()
            .unwrap()
            .to_string();

        let bind = app
            .clone()
            .oneshot(cookie_json_request(
                "POST",
                &format!("/api/admin/sites/{site_id}/bindings"),
                &cookie,
                json!({
                    "bindings": [
                        {
                            "node_id": node_ids[0],
                            "binding_role": "primary",
                            "priority": 10
                        },
                        {
                            "node_id": node_ids[1],
                            "binding_role": "standby",
                            "priority": 20
                        },
                        {
                            "node_id": node_ids[2],
                            "binding_role": "standby",
                            "priority": 30
                        }
                    ]
                }),
            ))
            .await
            .unwrap();
        assert_eq!(bind.status(), StatusCode::OK);

        let switch = app
            .clone()
            .oneshot(cookie_json_request(
                "POST",
                &format!("/api/admin/sites/{site_id}/switch-primary"),
                &cookie,
                json!({
                    "target_node_id": node_ids[2],
                    "reason": "manual failback test"
                }),
            ))
            .await
            .unwrap();
        assert_eq!(switch.status(), StatusCode::OK);
        let switch_body = response_json(switch).await;
        assert_eq!(
            switch_body["data"]["current_primary_node_id"]
                .as_str()
                .unwrap(),
            node_ids[2].as_str()
        );
        assert_eq!(
            switch_body["data"]["next_standby_node_id"]
                .as_str()
                .unwrap(),
            node_ids[0].as_str()
        );
    }

    #[tokio::test]
    #[ignore = "requires local postgres/redis stack from deploy/docker-compose.local.yml"]
    async fn postgres_redis_integration_flow() {
        let postgres_url = std::env::var("PINGORAHUB_TEST_POSTGRES_URL").unwrap_or_else(|_| {
            "postgres://postgres:postgres@127.0.0.1:5432/pingorahub".to_string()
        });
        let redis_url = std::env::var("PINGORAHUB_TEST_REDIS_URL")
            .unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());

        let pool = PgPool::connect(&postgres_url).await.unwrap();
        cleanup_database(&pool).await;
        let client = redis::Client::open(redis_url.as_str()).unwrap();
        let mut redis = client.get_multiplexed_async_connection().await.unwrap();
        let _: () = redis.flushdb().await.unwrap();

        let mut config = HubConfig::default();
        config.storage.backend = "postgres_redis".to_string();
        config.postgres.url = postgres_url;
        config.redis.url = redis_url;
        config.storage.redis_key_prefix = "pingorahub-test".to_string();

        let app = build_router(AppState {
            config: config.clone(),
            service: HubService::from_config(&config).await.unwrap(),
        });
        seed_admin_user_from_config(&config).await;
        let cookie = admin_session_cookie(&app).await;

        let create_node = app
            .clone()
            .oneshot(cookie_json_request(
                "POST",
                "/api/admin/nodes",
                &cookie,
                json!({
                    "node_code": "pg-cn-01",
                    "name": "pg node",
                    "region": "cn-east",
                    "idc": "lab",
                    "labels": { "role": "edge" }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(create_node.status(), StatusCode::OK);

        let create_site = app
            .clone()
            .oneshot(cookie_json_request(
                "POST",
                "/api/admin/sites",
                &cookie,
                json!({
                    "site_code": "pg-site",
                    "name": "pg-site",
                    "domain": "pg.example.com",
                    "listen_port": 443,
                    "protocol": "https",
                    "tls_enabled": true,
                    "config": { "upstreams": [] }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(create_site.status(), StatusCode::OK);
    }

    async fn build_test_app() -> Router {
        let service = HubService::new_memory();
        service
            .ensure_admin_user("admin", "平台管理员", "Admin#2026!")
            .await
            .unwrap();
        build_router(AppState {
            config: HubConfig::default(),
            service,
        })
    }

    async fn seed_admin_user_from_config(config: &HubConfig) {
        let service = HubService::from_config(config).await.unwrap();
        service
            .ensure_admin_user("admin", "平台管理员", "Admin#2026!")
            .await
            .unwrap();
    }

    async fn admin_session_cookie(app: &Router) -> String {
        let response = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/api/admin/auth/login",
                json!({
                    "username": "admin",
                    "password": "Admin#2026!"
                }),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        response
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .map(ToString::to_string)
            .unwrap()
    }

    fn json_request(method: &str, uri: &str, body: Value) -> axum::http::Request<Body> {
        axum::http::Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    fn cookie_json_request(
        method: &str,
        uri: &str,
        cookie: &str,
        body: Value,
    ) -> axum::http::Request<Body> {
        axum::http::Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .header("cookie", cookie)
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    fn auth_json_request(
        method: &str,
        uri: &str,
        token: &str,
        body: Value,
    ) -> axum::http::Request<Body> {
        axum::http::Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    fn empty_request(method: &str, uri: &str) -> axum::http::Request<Body> {
        axum::http::Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .unwrap()
    }

    fn cookie_empty_request(method: &str, uri: &str, cookie: &str) -> axum::http::Request<Body> {
        axum::http::Request::builder()
            .method(method)
            .uri(uri)
            .header("cookie", cookie)
            .body(Body::empty())
            .unwrap()
    }

    fn auth_empty_request(method: &str, uri: &str, token: &str) -> axum::http::Request<Body> {
        axum::http::Request::builder()
            .method(method)
            .uri(uri)
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap()
    }

    async fn response_json(response: axum::http::Response<Body>) -> Value {
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn cleanup_database(pool: &PgPool) {
        sqlx::query(
            r#"
            TRUNCATE TABLE
                admin_sessions,
                admin_users,
                certificate_orders,
                certificates,
                dns_zones,
                dns_providers,
                node_operations,
                failover_events,
                failover_policies,
                node_release_status,
                config_releases,
                site_node_bindings,
                site_configs,
                sites,
                node_heartbeats,
                node_credentials,
                nodes
            RESTART IDENTITY CASCADE
            "#,
        )
        .execute(pool)
        .await
        .unwrap();
    }
}
