use crate::{
    error::AppError,
    store::{
        AdminSessionContext, AdminSessionRecord, AdminUserRecord, CertificateOrderRecord,
        CertificateRecord, ClaimedNodeOperation, DnsProviderRecord, DnsZoneRecord, HeartbeatRecord,
        HubRepository, MemoryRepository, NodeAckRecord, NodeOperationRecord, NodeRecord,
        OperationTemplateRecord, PostgresRepository, ReleaseRecord, SiteBindingRecord, SiteRecord,
        StoreError, aggregate_release_status, node_status_name, protocol_name, site_status_name,
    },
    token_store::{IssuedTokens, MemoryTokenStore, NodeTokenStore, RedisTokenStore},
};
use anyhow::Result;
use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};
use chrono::{DateTime, Duration, Utc};
use pingorahub_application::ReleasePlanner;
use pingorahub_config_compiler::{
    CompiledConfigBundle, compile_site_bundle, compile_site_cleanup_bundle,
};
use pingorahub_dns_provider::{ProviderConfig, build_provider};
use pingorahub_domain::{
    NodeIdentity, NodeRuntimeState, NodeStatus, Protocol as SiteProtocol, ReleaseTarget, SiteStatus,
};
use pingorahub_infrastructure::HubConfig;
use pingorahub_ops_orchestrator::{OperationKind, OperationTemplate, OperationValidationError};
use pingorahub_protocol::{
    AdminLoginRequest, AdminLoginResponse, AdminMeResponse, AdminSessionInfo,
    CertificateOrderOverview, ChangeAdminPasswordRequest, ChangeAdminPasswordResponse,
    ConfigPackageResponse, CreateCertificateOrderRequest, CreateCertificateOrderResponse,
    CreateDnsProviderRequest, CreateDnsProviderResponse, CreateNodeOperationRequest,
    CreateNodeRequest, CreateNodeResponse, CreateOperationTemplateRequest, CreateReleaseRequest,
    CreateReleaseResponse, CreateSiteBindingsRequest, CreateSiteBindingsResponse,
    CreateSiteRequest, CreateSiteResponse, DeleteDnsProviderResponse, DeleteDnsZoneResponse,
    DeleteNodeResponse, DeleteOperationTemplateResponse, DeleteSiteResponse, DnsProviderOverview,
    DnsZoneOverview, LatestReleaseResponse, NodeDetail, NodeHeartbeatRequest,
    NodeHeartbeatResponse, NodeOperationOverview, NodeOverview, NodeRefreshRequest,
    NodeRefreshResponse, NodeRegisterRequest, NodeRegisterResponse, NodeSiteOverview,
    OperationResultRequest, OperationResultResponse, OperationTemplateOverview,
    PendingOperationResponse, ReleaseAckRequest, ReleaseAckResponse, ReleaseOverview,
    ReleaseStatusCounts, RenewSiteCertificateResponse, SiteBindingItem, SiteDetail, SiteOverview,
    SwitchSitePrimaryRequest, SwitchSitePrimaryResponse, SyncDnsZonesRequest, SyncDnsZonesResponse,
    UpdateDnsZoneStatusResponse, UpdateSiteRequest, UpdateSiteStatusResponse,
};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use uuid::Uuid;

const ADMIN_SESSION_TTL_HOURS: i64 = 8;
const CERT_AUTO_RENEW_BEFORE_DAYS: i64 = 30;

#[derive(Debug, Clone)]
struct SavedRelease {
    release_id: Uuid,
    release_version: String,
    config_hash: String,
    target_node_ids: Vec<Uuid>,
}

#[derive(Clone)]
pub struct HubService {
    repo: Arc<dyn HubRepository>,
    tokens: Arc<dyn NodeTokenStore>,
    release_counter: Arc<AtomicU64>,
    cert_auto_renew_before_days: i64,
}

impl HubService {
    pub fn new_memory() -> Self {
        Self::new_memory_with_cert_auto_renew_before_days(CERT_AUTO_RENEW_BEFORE_DAYS)
    }

    fn new_memory_with_cert_auto_renew_before_days(cert_auto_renew_before_days: i64) -> Self {
        Self {
            repo: Arc::new(MemoryRepository::default()),
            tokens: Arc::new(MemoryTokenStore::default()),
            release_counter: Arc::new(AtomicU64::new(1)),
            cert_auto_renew_before_days: normalize_cert_auto_renew_before_days(
                cert_auto_renew_before_days,
            ),
        }
    }

    pub async fn from_config(config: &HubConfig) -> Result<Self> {
        match config.storage.backend.as_str() {
            "memory" => Ok(Self::new_memory_with_cert_auto_renew_before_days(
                config.certificate.auto_renew_before_days,
            )),
            "postgres_redis" => {
                let repo = PostgresRepository::connect(
                    &config.postgres.url,
                    config.postgres.max_connections,
                )
                .await?;
                let tokens = RedisTokenStore::new(
                    &config.redis.url,
                    config.storage.redis_key_prefix.clone(),
                )?;
                Ok(Self {
                    repo: Arc::new(repo),
                    tokens: Arc::new(tokens),
                    release_counter: Arc::new(AtomicU64::new(1)),
                    cert_auto_renew_before_days: normalize_cert_auto_renew_before_days(
                        config.certificate.auto_renew_before_days,
                    ),
                })
            }
            _ => Ok(Self::new_memory()),
        }
    }

    pub async fn ensure_admin_user(
        &self,
        username: &str,
        display_name: &str,
        raw_password: &str,
    ) -> Result<bool, AppError> {
        let normalized_username = normalize_admin_username(username)?;
        if self
            .repo
            .admin_user_by_username(&normalized_username)
            .await
            .map_err(map_store_error)?
            .is_some()
        {
            return Ok(false);
        }

        let user = AdminUserRecord {
            user_id: Uuid::new_v4(),
            username: normalized_username,
            display_name: normalize_admin_display_name(display_name),
            password_hash: hash_password(raw_password)?,
            status: "active".to_string(),
            last_login_at: None,
            created_at: Utc::now(),
        };
        self.repo
            .save_admin_user(&user)
            .await
            .map_err(map_store_error)?;
        Ok(true)
    }

    pub async fn login_admin(
        &self,
        request: AdminLoginRequest,
    ) -> Result<(AdminLoginResponse, String), AppError> {
        let username = normalize_admin_username(&request.username)?;
        if request.password.is_empty() {
            return Err(AppError::bad_request("password is required"));
        }

        let user = self
            .repo
            .admin_user_by_username(&username)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| AppError::unauthorized("invalid username or password"))?;
        if user.status != "active" {
            return Err(AppError::unauthorized("admin account is disabled"));
        }
        verify_password(&user.password_hash, &request.password)?;

        let raw_session_token = generate_admin_session_token();
        let session = AdminSessionRecord {
            session_id: Uuid::new_v4(),
            user_id: user.user_id,
            session_token_hash: hash_session_token(&raw_session_token),
            expires_at: Utc::now() + Duration::hours(ADMIN_SESSION_TTL_HOURS),
            created_at: Utc::now(),
            last_seen_at: Utc::now(),
        };
        self.repo
            .save_admin_session(&session)
            .await
            .map_err(map_store_error)?;
        self.repo
            .touch_admin_user_login(user.user_id, Utc::now())
            .await
            .map_err(map_store_error)?;

        Ok((
            AdminLoginResponse {
                session: AdminSessionInfo {
                    user_id: user.user_id,
                    username: user.username,
                    display_name: user.display_name,
                    expires_at: session.expires_at,
                },
            },
            raw_session_token,
        ))
    }

    pub async fn authorize_admin_session(
        &self,
        session_token: &str,
    ) -> Result<AdminSessionInfo, AppError> {
        if session_token.trim().is_empty() {
            return Err(AppError::unauthorized("missing admin session"));
        }

        let session_hash = hash_session_token(session_token);
        let session = self
            .repo
            .admin_session_by_token_hash(&session_hash)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| AppError::unauthorized("invalid admin session"))?;
        validate_admin_session(&session)?;
        self.repo
            .touch_admin_session(&session_hash, Utc::now())
            .await
            .map_err(map_store_error)?;

        Ok(AdminSessionInfo {
            user_id: session.user_id,
            username: session.username,
            display_name: session.display_name,
            expires_at: session.expires_at,
        })
    }

    pub async fn current_admin(&self, session_token: &str) -> Result<AdminMeResponse, AppError> {
        let session = self.authorize_admin_session(session_token).await?;
        Ok(AdminMeResponse { session })
    }

    pub async fn change_admin_password(
        &self,
        session_token: &str,
        request: ChangeAdminPasswordRequest,
    ) -> Result<ChangeAdminPasswordResponse, AppError> {
        let session = self.authorize_admin_session(session_token).await?;
        if request.current_password.is_empty() {
            return Err(AppError::bad_request("current_password is required"));
        }
        if request.new_password.is_empty() {
            return Err(AppError::bad_request("new_password is required"));
        }
        if request.current_password == request.new_password {
            return Err(AppError::bad_request(
                "new password must be different from current password",
            ));
        }

        let mut user = self
            .repo
            .admin_user_by_username(&session.username)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| AppError::unauthorized("admin account not found"))?;
        if user.status != "active" {
            return Err(AppError::unauthorized("admin account is disabled"));
        }

        verify_password(&user.password_hash, &request.current_password)?;
        user.password_hash = hash_password(&request.new_password)?;
        self.repo
            .save_admin_user(&user)
            .await
            .map_err(map_store_error)?;
        self.repo
            .delete_admin_sessions_for_user(user.user_id, Some(&hash_session_token(session_token)))
            .await
            .map_err(map_store_error)?;

        Ok(ChangeAdminPasswordResponse {
            user_id: user.user_id,
            username: user.username,
            changed: true,
        })
    }

    pub async fn logout_admin(&self, session_token: &str) -> Result<(), AppError> {
        if session_token.trim().is_empty() {
            return Ok(());
        }
        self.repo
            .delete_admin_session(&hash_session_token(session_token))
            .await
            .map_err(map_store_error)?;
        Ok(())
    }

    pub async fn create_node(
        &self,
        request: CreateNodeRequest,
    ) -> Result<CreateNodeResponse, AppError> {
        if request.node_code.trim().is_empty() || request.name.trim().is_empty() {
            return Err(AppError::bad_request("node_code and name are required"));
        }

        if self
            .repo
            .node_id_by_code(&request.node_code)
            .await
            .map_err(map_store_error)?
            .is_some()
        {
            return Err(AppError::conflict("node_code already exists"));
        }

        let node_id = Uuid::new_v4();
        let bootstrap_token = format!("boot-{}", Uuid::new_v4());
        let bootstrap_token_hash = hash_bootstrap_token(&bootstrap_token);
        let node = NodeRecord {
            identity: NodeIdentity {
                id: node_id,
                node_code: request.node_code,
                name: request.name,
                region: request.region,
                idc: request.idc,
                status: NodeStatus::Pending,
            },
            runtime: NodeRuntimeState {
                active_config_version: None,
                last_seen_at: None,
                site_count: 0,
                health_score: 100,
            },
            labels: request.labels,
            hostname: None,
            public_ip: None,
            private_ip: None,
            bootstrap_token: Some(bootstrap_token_hash),
            pingora_version: None,
            agent_version: None,
        };
        self.repo.save_node(&node).await.map_err(map_store_error)?;

        Ok(CreateNodeResponse {
            node_id,
            bootstrap_token,
        })
    }

    pub async fn list_nodes(&self) -> Result<Vec<NodeOverview>, AppError> {
        let mut nodes: Vec<NodeOverview> = self
            .repo
            .list_nodes()
            .await
            .map_err(map_store_error)?
            .into_iter()
            .map(|node| NodeOverview {
                node_id: node.identity.id,
                node_code: node.identity.node_code,
                name: node.identity.name,
                region: node.identity.region,
                idc: node.identity.idc,
                labels: node.labels,
                status: node_status_name(node.identity.status),
                active_config_version: node.runtime.active_config_version,
                last_seen_at: node.runtime.last_seen_at,
            })
            .collect();
        nodes.sort_by(|left, right| left.node_code.cmp(&right.node_code));
        Ok(nodes)
    }

    pub async fn node_detail(&self, node_id: Uuid) -> Result<NodeDetail, AppError> {
        let node = self
            .repo
            .node(node_id)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| AppError::not_found("node not found"))?;
        let mut sites = self
            .repo
            .list_sites()
            .await
            .map_err(map_store_error)?
            .into_iter()
            .filter_map(|site| {
                let (binding_role, priority) = site
                    .bindings
                    .iter()
                    .find(|binding| binding.node_id == node_id)
                    .map(|binding| (binding.binding_role.clone(), binding.priority))?;
                Some(NodeSiteOverview {
                    site_id: site.id,
                    site_code: site.site_code,
                    name: site.name,
                    domain: site.domain,
                    status: site_status_name(site.status),
                    version: site.version,
                    binding_role,
                    priority,
                })
            })
            .collect::<Vec<_>>();
        sites.sort_by(|left, right| {
            left.priority
                .cmp(&right.priority)
                .then_with(|| left.site_code.cmp(&right.site_code))
        });

        Ok(NodeDetail {
            node_id: node.identity.id,
            node_code: node.identity.node_code,
            name: node.identity.name,
            region: node.identity.region,
            idc: node.identity.idc,
            labels: node.labels,
            status: node_status_name(node.identity.status),
            hostname: node.hostname,
            public_ip: node.public_ip,
            private_ip: node.private_ip,
            pingora_version: node.pingora_version,
            agent_version: node.agent_version,
            active_config_version: node.runtime.active_config_version,
            last_seen_at: node.runtime.last_seen_at,
            runtime_site_count: node.runtime.site_count,
            health_score: node.runtime.health_score,
            sites,
        })
    }

    pub async fn delete_node(&self, node_id: Uuid) -> Result<DeleteNodeResponse, AppError> {
        let node = self
            .repo
            .node(node_id)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| AppError::not_found("node not found"))?;

        let guard = self
            .repo
            .node_delete_guard(node_id)
            .await
            .map_err(map_store_error)?;

        if !guard.bound_site_codes.is_empty() {
            return Err(AppError::conflict(format!(
                "node is still bound to site(s): {}",
                guard.bound_site_codes.join(", ")
            )));
        }

        if guard.failover_policy_count > 0 {
            return Err(AppError::conflict(format!(
                "node is still referenced by {} failover policy entries",
                guard.failover_policy_count
            )));
        }

        self.tokens
            .revoke_tokens(node_id)
            .await
            .map_err(map_store_error)?;
        self.repo
            .delete_node(node_id)
            .await
            .map_err(map_store_error)?;

        Ok(DeleteNodeResponse {
            node_id,
            node_code: node.identity.node_code,
            deleted: true,
        })
    }

    pub async fn create_operation_template(
        &self,
        request: CreateOperationTemplateRequest,
    ) -> Result<OperationTemplateOverview, AppError> {
        let name = normalize_operation_template_name(&request.name)?;
        let operation_type = normalize_operation_type(&request.operation_type)?;
        let command_template = request.command_template.trim().to_string();
        if command_template.is_empty() {
            return Err(AppError::bad_request("command_template is required"));
        }

        let allowed_params = normalize_allowed_params(&request.allowed_params)?;
        let timeout_seconds = request.timeout_seconds.max(1);
        let run_as_user = normalize_operation_run_as_user(&request.run_as_user);
        let existing = self
            .repo
            .operation_template_by_name(&name)
            .await
            .map_err(map_store_error)?;

        let template = OperationTemplateRecord {
            template_id: existing
                .as_ref()
                .map(|template| template.template_id)
                .unwrap_or_else(Uuid::new_v4),
            name,
            operation_type,
            command_template,
            allowed_params: json!(allowed_params),
            timeout_seconds,
            run_as_user,
            approval_required: request.approval_required,
            created_at: existing
                .as_ref()
                .map(|template| template.created_at)
                .unwrap_or_else(Utc::now),
        };
        self.repo
            .save_operation_template(&template)
            .await
            .map_err(map_store_error)?;
        operation_template_overview(&template)
    }

    pub async fn update_operation_template(
        &self,
        template_id: Uuid,
        request: CreateOperationTemplateRequest,
    ) -> Result<OperationTemplateOverview, AppError> {
        let existing = self
            .repo
            .operation_template(template_id)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| AppError::not_found("operation template not found"))?;
        let name = normalize_operation_template_name(&request.name)?;
        let operation_type = normalize_operation_type(&request.operation_type)?;
        let command_template = request.command_template.trim().to_string();
        if command_template.is_empty() {
            return Err(AppError::bad_request("command_template is required"));
        }

        let duplicated = self
            .repo
            .operation_template_by_name(&name)
            .await
            .map_err(map_store_error)?;
        if duplicated
            .as_ref()
            .map(|template| template.template_id != template_id)
            .unwrap_or(false)
        {
            return Err(AppError::conflict("operation template name already exists"));
        }

        let template = OperationTemplateRecord {
            template_id,
            name,
            operation_type,
            command_template,
            allowed_params: json!(normalize_allowed_params(&request.allowed_params)?),
            timeout_seconds: request.timeout_seconds.max(1),
            run_as_user: normalize_operation_run_as_user(&request.run_as_user),
            approval_required: request.approval_required,
            created_at: existing.created_at,
        };
        self.repo
            .save_operation_template(&template)
            .await
            .map_err(map_store_error)?;
        operation_template_overview(&template)
    }

    pub async fn delete_operation_template(
        &self,
        template_id: Uuid,
    ) -> Result<DeleteOperationTemplateResponse, AppError> {
        let template = self
            .repo
            .operation_template(template_id)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| AppError::not_found("operation template not found"))?;

        match self.repo.delete_operation_template(template_id).await {
            Ok(()) => Ok(DeleteOperationTemplateResponse {
                template_id,
                name: template.name,
                deleted: true,
            }),
            Err(error) if is_foreign_key_violation(&error) => Err(AppError::conflict(
                "operation template is still referenced by node operations",
            )),
            Err(error) => Err(map_store_error(error)),
        }
    }

    pub async fn list_operation_templates(
        &self,
    ) -> Result<Vec<OperationTemplateOverview>, AppError> {
        let mut templates = self
            .repo
            .list_operation_templates()
            .await
            .map_err(map_store_error)?
            .into_iter()
            .map(|template| operation_template_overview(&template))
            .collect::<Result<Vec<_>, _>>()?;
        templates.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(templates)
    }

    pub async fn create_node_operation(
        &self,
        node_id: Uuid,
        request: CreateNodeOperationRequest,
        requested_by: &str,
    ) -> Result<NodeOperationOverview, AppError> {
        self.repo
            .node(node_id)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| AppError::not_found("node not found"))?;
        let template = self
            .repo
            .operation_template(request.template_id)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| AppError::not_found("operation template not found"))?;

        let input_params = normalize_operation_input_params(&request.input_params)?;
        build_operation_template(&template)?
            .render_command(&input_params)
            .map_err(|error: OperationValidationError| AppError::bad_request(error.to_string()))?;

        let approval_ticket = request
            .approval_ticket
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        let exec_status = if template.approval_required && approval_ticket.is_none() {
            "pending".to_string()
        } else {
            "approved".to_string()
        };
        let approved_by = if exec_status == "approved" {
            Some(
                approval_ticket
                    .clone()
                    .unwrap_or_else(|| requested_by.to_string()),
            )
        } else {
            None
        };

        let operation = NodeOperationRecord {
            operation_id: Uuid::new_v4(),
            node_id,
            template_id: template.template_id,
            event_id: None,
            input_params: Value::Object(input_params),
            exec_status,
            requested_by: requested_by.to_string(),
            approved_by,
            exit_code: None,
            stdout_log: None,
            stderr_log: None,
            started_at: None,
            finished_at: None,
            created_at: Utc::now(),
        };
        self.repo
            .save_node_operation(&operation)
            .await
            .map_err(map_store_error)?;
        node_operation_overview(&operation, &template)
    }

    pub async fn list_node_operations(
        &self,
        node_id: Uuid,
    ) -> Result<Vec<NodeOperationOverview>, AppError> {
        self.repo
            .node(node_id)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| AppError::not_found("node not found"))?;

        let operations = self
            .repo
            .list_node_operations(node_id)
            .await
            .map_err(map_store_error)?;
        let mut overviews = Vec::with_capacity(operations.len());
        for operation in operations {
            let template = self
                .repo
                .operation_template(operation.template_id)
                .await
                .map_err(map_store_error)?
                .ok_or_else(|| AppError::internal("operation template missing"))?;
            overviews.push(node_operation_overview(&operation, &template)?);
        }
        Ok(overviews)
    }

    pub async fn register_node(
        &self,
        request: NodeRegisterRequest,
    ) -> Result<NodeRegisterResponse, AppError> {
        let node_id = self
            .repo
            .node_id_by_code(&request.node_code)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| AppError::not_found("node_code not found"))?;
        let mut node = self
            .repo
            .node(node_id)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| AppError::not_found("node not found"))?;

        let expected = node
            .bootstrap_token
            .clone()
            .ok_or_else(|| AppError::unauthorized("bootstrap token already used or unavailable"))?;
        if expected != hash_bootstrap_token(&request.bootstrap_token) {
            return Err(AppError::unauthorized("invalid bootstrap token"));
        }

        let issued = self.issue_node_tokens(node_id).await?;
        node.identity.status = NodeStatus::Online;
        node.hostname = Some(request.hostname);
        node.public_ip = request.public_ip;
        node.private_ip = request.private_ip;
        node.agent_version = Some(request.agent_version);
        node.bootstrap_token = None;
        node.runtime.last_seen_at = Some(Utc::now());
        self.repo.save_node(&node).await.map_err(map_store_error)?;

        Ok(NodeRegisterResponse {
            node_id,
            access_token: issued.access_token,
            refresh_token: issued.refresh_token,
            expires_at: issued.expires_at,
        })
    }

    pub async fn refresh_node_token(
        &self,
        request: NodeRefreshRequest,
    ) -> Result<NodeRefreshResponse, AppError> {
        let lease = self
            .tokens
            .refresh_lease(&request.refresh_token)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| AppError::unauthorized("invalid refresh token"))?;
        if lease.node_id != request.node_id {
            return Err(AppError::unauthorized("refresh token does not match node"));
        }

        let issued = self.issue_node_tokens(request.node_id).await?;

        Ok(NodeRefreshResponse {
            node_id: request.node_id,
            access_token: issued.access_token,
            refresh_token: issued.refresh_token,
            expires_at: issued.expires_at,
        })
    }

    pub async fn authorize_node(&self, node_id: Uuid, access_token: &str) -> Result<(), AppError> {
        let lease = self
            .tokens
            .access_lease(access_token)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| AppError::unauthorized("invalid access token"))?;
        if lease.node_id != node_id {
            return Err(AppError::unauthorized("access token does not match node"));
        }
        if lease.expires_at < Utc::now() {
            return Err(AppError::unauthorized("access token expired"));
        }
        if self
            .repo
            .node(node_id)
            .await
            .map_err(map_store_error)?
            .is_none()
        {
            return Err(AppError::not_found("node not found"));
        }
        Ok(())
    }

    pub async fn claim_next_node_operation(
        &self,
        node_id: Uuid,
    ) -> Result<Option<PendingOperationResponse>, AppError> {
        let claimed = self
            .repo
            .claim_next_node_operation(node_id)
            .await
            .map_err(map_store_error)?;

        claimed.map(claimed_node_operation_dispatch).transpose()
    }

    pub async fn report_node_operation_result(
        &self,
        operation_id: Uuid,
        request: OperationResultRequest,
    ) -> Result<OperationResultResponse, AppError> {
        let mut operation = self
            .repo
            .node_operation(operation_id)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| AppError::not_found("node operation not found"))?;
        if operation.node_id != request.node_id {
            return Err(AppError::unauthorized("operation does not belong to node"));
        }

        let exec_status = normalize_operation_exec_status(&request.exec_status)?;
        operation.exec_status = exec_status.clone();
        operation.exit_code = Some(request.exit_code);
        operation.stdout_log = Some(truncate_log_output(&request.stdout));
        operation.stderr_log = Some(truncate_log_output(&request.stderr));
        operation.finished_at = Some(request.finished_at);
        if operation.started_at.is_none() {
            operation.started_at = Some(Utc::now());
        }

        self.repo
            .update_node_operation(&operation)
            .await
            .map_err(map_store_error)?;

        Ok(OperationResultResponse {
            operation_id,
            node_id: request.node_id,
            exec_status,
        })
    }

    pub async fn node_heartbeat(
        &self,
        request: NodeHeartbeatRequest,
    ) -> Result<NodeHeartbeatResponse, AppError> {
        if self
            .repo
            .node(request.node_id)
            .await
            .map_err(map_store_error)?
            .is_none()
        {
            return Err(AppError::not_found("node not found"));
        }

        self.repo
            .record_heartbeat(&HeartbeatRecord {
                node_id: request.node_id,
                pingora_version: request.pingora_version,
                agent_version: request.agent_version,
                active_config_version: request.active_config_version,
                site_count: request.site_count,
                cpu_usage: request.cpu_usage,
                mem_usage: request.mem_usage,
                disk_usage: request.disk_usage,
                health_score: request.health_score,
                reported_at: Utc::now(),
            })
            .await
            .map_err(map_store_error)?;

        Ok(NodeHeartbeatResponse {
            server_time: Utc::now(),
            next_heartbeat_after_seconds: 15,
        })
    }

    pub async fn create_site(
        &self,
        request: CreateSiteRequest,
    ) -> Result<CreateSiteResponse, AppError> {
        if request.domain.trim().is_empty() {
            return Err(AppError::bad_request("domain is required"));
        }

        let protocol = parse_protocol(&request.protocol)?;
        let site_code = generate_unique_site_code(
            &self.repo,
            request.site_code.as_deref(),
            &request.name,
            &request.domain,
        )
        .await?;

        let site_id = Uuid::new_v4();
        let site = SiteRecord {
            id: site_id,
            site_code: site_code.clone(),
            name: request.name,
            domain: request.domain,
            listen_port: request.listen_port,
            protocol,
            tls_enabled: request.tls_enabled,
            status: SiteStatus::Draft,
            version: 1,
            config: request.config,
            bindings: Vec::new(),
        };
        self.repo.save_site(&site).await.map_err(map_store_error)?;

        Ok(CreateSiteResponse {
            site_id,
            site_code,
            version: site.version,
            status: site_status_name(site.status),
        })
    }

    pub async fn update_site(
        &self,
        site_id: Uuid,
        request: UpdateSiteRequest,
    ) -> Result<CreateSiteResponse, AppError> {
        let protocol = parse_protocol(&request.protocol)?;
        let mut site = self
            .repo
            .site(site_id)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| AppError::not_found("site not found"))?;

        site.name = request.name;
        site.domain = request.domain;
        site.listen_port = request.listen_port;
        site.protocol = protocol;
        site.tls_enabled = request.tls_enabled;
        site.config = request.config;
        site.version += 1;
        site.status = SiteStatus::Draft;
        self.repo.save_site(&site).await.map_err(map_store_error)?;

        Ok(CreateSiteResponse {
            site_id,
            site_code: site.site_code,
            version: site.version,
            status: site_status_name(site.status),
        })
    }

    pub async fn update_site_status(
        &self,
        site_id: Uuid,
        enabled: bool,
    ) -> Result<UpdateSiteStatusResponse, AppError> {
        let mut site = self
            .repo
            .site(site_id)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| AppError::not_found("site not found"))?;

        site.status = if enabled {
            if site.bindings.is_empty() {
                SiteStatus::Draft
            } else {
                SiteStatus::Published
            }
        } else {
            SiteStatus::Disabled
        };
        site.version += 1;
        self.repo.save_site(&site).await.map_err(map_store_error)?;
        if !site.bindings.is_empty() {
            let release_type = if enabled { "enable" } else { "disable" };
            let reason = if enabled {
                format!("enable site {}", site.site_code)
            } else {
                format!("disable site {}", site.site_code)
            };
            let _ = self
                .save_site_release(&site, release_type.to_string(), reason)
                .await?;
        }

        Ok(UpdateSiteStatusResponse {
            site_id,
            site_code: site.site_code,
            status: site_status_name(site.status),
            version: site.version,
        })
    }

    pub async fn delete_site(&self, site_id: Uuid) -> Result<DeleteSiteResponse, AppError> {
        let site = self
            .repo
            .site(site_id)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| AppError::not_found("site not found"))?;

        if !site.bindings.is_empty() {
            let _ = self
                .save_site_cleanup_release(
                    &site,
                    site.bindings
                        .iter()
                        .map(|binding| binding.node_id)
                        .collect(),
                    "delete site".to_string(),
                )
                .await?;
        }

        self.repo
            .delete_site(site_id)
            .await
            .map_err(map_store_error)?;

        Ok(DeleteSiteResponse {
            site_id,
            site_code: site.site_code,
            deleted: true,
        })
    }

    pub async fn list_sites(&self) -> Result<Vec<SiteOverview>, AppError> {
        let node_lookup = self.node_lookup().await?;
        let mut sites: Vec<SiteOverview> = self
            .repo
            .list_sites()
            .await
            .map_err(map_store_error)?
            .into_iter()
            .map(|site| {
                let primary_node = primary_site_binding(&site)
                    .and_then(|binding| node_lookup.get(&binding.node_id));
                SiteOverview {
                    site_id: site.id,
                    site_code: site.site_code,
                    name: site.name,
                    domain: site.domain,
                    protocol: protocol_name(site.protocol).to_string(),
                    status: site_status_name(site.status),
                    version: site.version,
                    binding_count: site.bindings.len(),
                    primary_node_id: primary_node.map(|node| node.identity.id),
                    primary_node_code: primary_node.map(|node| node.identity.node_code.clone()),
                    primary_node_name: primary_node.map(|node| node.identity.name.clone()),
                    primary_node_status: primary_node
                        .map(|node| node_status_name(node.identity.status)),
                }
            })
            .collect();
        sites.sort_by(|left, right| left.site_code.cmp(&right.site_code));
        Ok(sites)
    }

    pub async fn site_detail(&self, site_id: Uuid) -> Result<SiteDetail, AppError> {
        let node_lookup = self.node_lookup().await?;
        let site = self
            .repo
            .site(site_id)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| AppError::not_found("site not found"))?;

        Ok(SiteDetail {
            site_id: site.id,
            site_code: site.site_code,
            name: site.name,
            domain: site.domain,
            listen_port: site.listen_port,
            protocol: protocol_name(site.protocol).to_string(),
            tls_enabled: site.tls_enabled,
            status: site_status_name(site.status),
            version: site.version,
            config: site.config,
            bindings: site
                .bindings
                .into_iter()
                .map(|binding| site_binding_item(binding, &node_lookup))
                .collect(),
        })
    }

    async fn node_lookup(&self) -> Result<HashMap<Uuid, NodeRecord>, AppError> {
        Ok(self
            .repo
            .list_nodes()
            .await
            .map_err(map_store_error)?
            .into_iter()
            .map(|node| (node.identity.id, node))
            .collect())
    }

    pub async fn bind_site_nodes(
        &self,
        site_id: Uuid,
        request: CreateSiteBindingsRequest,
    ) -> Result<CreateSiteBindingsResponse, AppError> {
        let bindings = normalize_site_bindings(request.bindings)?;
        for binding in &bindings {
            if self
                .repo
                .node(binding.node_id)
                .await
                .map_err(map_store_error)?
                .is_none()
            {
                return Err(AppError::not_found(format!(
                    "node {} not found",
                    binding.node_id
                )));
            }
        }

        let mut site = self
            .repo
            .site(site_id)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| AppError::not_found("site not found"))?;
        site.bindings = bindings;
        self.repo.save_site(&site).await.map_err(map_store_error)?;

        Ok(CreateSiteBindingsResponse {
            site_id,
            binding_count: site.bindings.len(),
        })
    }

    pub async fn renew_site_certificate(
        &self,
        site_id: Uuid,
    ) -> Result<RenewSiteCertificateResponse, AppError> {
        let site = self
            .repo
            .site(site_id)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| AppError::not_found("site not found"))?;
        if !site.tls_enabled || site.protocol != SiteProtocol::Https {
            return Err(AppError::bad_request(
                "only https/tls-enabled sites can renew certificates",
            ));
        }

        let certificate_id = self
            .repo
            .site_certificates(site_id)
            .await
            .map_err(map_store_error)?
            .first()
            .map(|certificate| certificate.certificate_id);

        let active_order = self
            .repo
            .list_certificate_orders()
            .await
            .map_err(map_store_error)?
            .into_iter()
            .find(|order| {
                (order.site_id == Some(site_id)
                    || (certificate_id.is_some() && order.certificate_id == certificate_id))
                    && !matches!(
                        order.order_status.as_str(),
                        "issued" | "dns_challenge_failed" | "issue_failed"
                    )
            });
        if let Some(order) = active_order {
            return Err(AppError::conflict(format!(
                "site already has an active certificate order {} in status {}",
                order.order_id, order.order_status
            )));
        }

        let common_name = normalize_certificate_identifier(&site.domain)?;
        let zone = self
            .repo
            .list_dns_zones()
            .await
            .map_err(map_store_error)?
            .into_iter()
            .filter(|zone| {
                zone.status == "active" && domain_matches_zone(&common_name, &zone.zone_name)
            })
            .max_by_key(|zone| zone.zone_name.len())
            .ok_or_else(|| AppError::not_found("no matching dns zone found for site domain"))?;

        let response = self
            .issue_certificate_order(
                Some(site.id),
                certificate_id,
                common_name.clone(),
                vec![common_name],
                zone.zone_id,
                &zone.zone_name,
                "letsencrypt-production".to_string(),
                "renew",
            )
            .await?;

        Ok(RenewSiteCertificateResponse {
            site_id,
            domain: site.domain,
            zone_id: zone.zone_id,
            order_id: response.order_id,
            certificate_id: response.certificate_id,
            order_status: response.order_status,
        })
    }

    pub async fn switch_site_primary(
        &self,
        site_id: Uuid,
        request: SwitchSitePrimaryRequest,
    ) -> Result<SwitchSitePrimaryResponse, AppError> {
        let mut site = self
            .repo
            .site(site_id)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| AppError::not_found("site not found"))?;
        if site.bindings.is_empty() {
            return Err(AppError::bad_request("site has no node bindings"));
        }

        let current_primary = site
            .bindings
            .iter()
            .find(|binding| binding.binding_role == "primary")
            .cloned()
            .or_else(|| site.bindings.first().cloned());
        let current_primary_node_id = current_primary.as_ref().map(|binding| binding.node_id);

        let target_node_id = if let Some(target_node_id) = request.target_node_id {
            if self
                .repo
                .node(target_node_id)
                .await
                .map_err(map_store_error)?
                .is_none()
            {
                return Err(AppError::not_found("target node not found"));
            }
            target_node_id
        } else {
            site.bindings
                .iter()
                .filter(|binding| Some(binding.node_id) != current_primary_node_id)
                .find(|binding| binding.binding_role == "standby")
                .or_else(|| {
                    site.bindings
                        .iter()
                        .find(|binding| Some(binding.node_id) != current_primary_node_id)
                })
                .map(|binding| binding.node_id)
                .ok_or_else(|| AppError::conflict("site has no standby node to switch to"))?
        };

        if current_primary_node_id == Some(target_node_id) {
            return Err(AppError::conflict(
                "target node is already the primary node",
            ));
        }

        let keep_previous_as_standby = request.keep_previous_as_standby.unwrap_or(true);
        let previous_primary_node_id = current_primary_node_id;
        let mut new_bindings = vec![SiteBindingRecord {
            node_id: target_node_id,
            binding_role: "primary".to_string(),
            priority: 100,
        }];

        let mut next_priority = 200;
        if keep_previous_as_standby {
            if let Some(previous_primary_node_id) = previous_primary_node_id {
                if previous_primary_node_id != target_node_id {
                    new_bindings.push(SiteBindingRecord {
                        node_id: previous_primary_node_id,
                        binding_role: "standby".to_string(),
                        priority: next_priority,
                    });
                    next_priority += 100;
                }
            }
        }

        let mut remaining = site.bindings.clone();
        remaining.sort_by_key(|binding| binding.priority);
        for binding in remaining {
            if binding.node_id == target_node_id
                || Some(binding.node_id) == previous_primary_node_id
            {
                continue;
            }
            let role = if binding.binding_role == "primary" {
                "standby".to_string()
            } else {
                binding.binding_role
            };
            new_bindings.push(SiteBindingRecord {
                node_id: binding.node_id,
                binding_role: role,
                priority: next_priority,
            });
            next_priority += 100;
        }

        site.bindings = new_bindings;
        site.status = SiteStatus::Draft;
        site.version += 1;
        self.repo.save_site(&site).await.map_err(map_store_error)?;

        let release = self
            .create_release(CreateReleaseRequest {
                scope_type: "site".to_string(),
                scope_id: site_id,
                release_type: "switch".to_string(),
                reason: request.reason.unwrap_or_else(|| {
                    format!(
                        "switch primary node for {} to {}",
                        site.site_code, target_node_id
                    )
                }),
            })
            .await?;

        Ok(SwitchSitePrimaryResponse {
            site_id,
            previous_primary_node_id,
            current_primary_node_id: target_node_id,
            binding_count: site.bindings.len(),
            release_id: release.release_id,
            release_version: release.release_version,
        })
    }

    pub async fn create_release(
        &self,
        request: CreateReleaseRequest,
    ) -> Result<CreateReleaseResponse, AppError> {
        if request.scope_type != "site" {
            return Err(AppError::bad_request(
                "only site scope is implemented in P0/P1",
            ));
        }

        let mut site = self
            .repo
            .site(request.scope_id)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| AppError::not_found("site not found"))?;
        let target_node_ids: Vec<Uuid> = site
            .bindings
            .iter()
            .map(|binding| binding.node_id)
            .collect();
        let detached_node_ids = self
            .detached_node_ids_for_site(site.id, &target_node_ids)
            .await?;
        if target_node_ids.is_empty() && detached_node_ids.is_empty() {
            return Err(AppError::bad_request("site has no node bindings"));
        }

        let primary_release = if target_node_ids.is_empty() {
            None
        } else {
            Some(
                self.save_site_release(&site, request.release_type.clone(), request.reason.clone())
                    .await?,
            )
        };
        let cleanup_release = if detached_node_ids.is_empty() {
            None
        } else {
            Some(
                self.save_site_cleanup_release(&site, detached_node_ids, request.reason.clone())
                    .await?,
            )
        };

        site.status = if primary_release.is_some() {
            SiteStatus::Published
        } else {
            SiteStatus::Draft
        };
        self.repo.save_site(&site).await.map_err(map_store_error)?;

        let release = primary_release
            .or(cleanup_release)
            .ok_or_else(|| AppError::internal("release creation produced no artifacts"))?;
        Ok(release.into_response())
    }

    async fn detached_node_ids_for_site(
        &self,
        site_id: Uuid,
        current_target_node_ids: &[Uuid],
    ) -> Result<Vec<Uuid>, AppError> {
        let current_target_node_ids = current_target_node_ids
            .iter()
            .copied()
            .collect::<HashSet<_>>();
        let previous_release = self
            .repo
            .list_releases()
            .await
            .map_err(map_store_error)?
            .into_iter()
            .filter(|release| release.scope_type == "site" && release.scope_id == Some(site_id))
            .max_by(|left, right| {
                left.created_at
                    .cmp(&right.created_at)
                    .then_with(|| left.release_version.cmp(&right.release_version))
            });

        let mut detached = previous_release
            .map(|release| {
                release
                    .target_node_ids
                    .into_iter()
                    .filter(|node_id| !current_target_node_ids.contains(node_id))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        detached.sort();
        detached.dedup();
        Ok(detached)
    }

    async fn save_site_release(
        &self,
        site: &SiteRecord,
        release_type: String,
        reason: String,
    ) -> Result<SavedRelease, AppError> {
        let serial = self.release_counter.fetch_add(1, Ordering::Relaxed);
        let release_version = ReleasePlanner::build_release_version("site", Utc::now(), serial);
        let target_node_ids = site
            .bindings
            .iter()
            .map(|binding| binding.node_id)
            .collect::<Vec<_>>();
        let certificates = self
            .repo
            .site_certificates(site.id)
            .await
            .map_err(map_store_error)?;
        let targets = site
            .bindings
            .iter()
            .map(|binding| ReleaseTarget {
                node_id: binding.node_id,
                site_ids: vec![site.id],
                preheated: binding.binding_role == "standby",
            })
            .collect();
        let bundle = compile_site_bundle(site.to_spec(), certificates, targets, release_version)
            .map_err(|error| AppError::internal(error.to_string()))?;

        self.save_compiled_release(
            "site".to_string(),
            Some(site.id),
            release_type,
            reason,
            bundle,
            target_node_ids,
        )
        .await
    }

    async fn save_site_cleanup_release(
        &self,
        site: &SiteRecord,
        detached_node_ids: Vec<Uuid>,
        reason: String,
    ) -> Result<SavedRelease, AppError> {
        let serial = self.release_counter.fetch_add(1, Ordering::Relaxed);
        let release_version = ReleasePlanner::build_release_version("cleanup", Utc::now(), serial);
        let targets = detached_node_ids
            .iter()
            .map(|node_id| ReleaseTarget {
                node_id: *node_id,
                site_ids: vec![site.id],
                preheated: false,
            })
            .collect();
        let bundle = compile_site_cleanup_bundle(site.to_spec(), targets, release_version)
            .map_err(|error| AppError::internal(error.to_string()))?;

        self.save_compiled_release(
            "site_cleanup".to_string(),
            Some(site.id),
            "cleanup".to_string(),
            format!("cleanup detached nodes for {}: {}", site.site_code, reason),
            bundle,
            detached_node_ids,
        )
        .await
    }

    async fn save_compiled_release(
        &self,
        scope_type: String,
        scope_id: Option<Uuid>,
        release_type: String,
        reason: String,
        bundle: CompiledConfigBundle,
        target_node_ids: Vec<Uuid>,
    ) -> Result<SavedRelease, AppError> {
        let release_id = bundle.manifest.release_id;
        let release_version = bundle.manifest.release_version.clone();
        let config_hash = bundle.manifest.config_hash.clone();
        let ack_status = build_release_ack_status(&target_node_ids);
        let release = ReleaseRecord {
            release_id,
            scope_type,
            scope_id,
            release_type,
            release_version: release_version.clone(),
            reason,
            status: aggregate_release_status(&ack_status),
            bundle,
            target_node_ids: target_node_ids.clone(),
            ack_status,
            created_at: Utc::now(),
        };
        self.repo
            .save_release(&release)
            .await
            .map_err(map_store_error)?;

        Ok(SavedRelease {
            release_id,
            release_version,
            config_hash,
            target_node_ids,
        })
    }

    pub async fn list_releases(&self) -> Result<Vec<ReleaseOverview>, AppError> {
        let releases = self.repo.list_releases().await.map_err(map_store_error)?;
        let mut overviews = releases
            .into_iter()
            .map(|release| {
                let counts = summarize_release_counts(&release.ack_status);
                ReleaseOverview {
                    release_id: release.release_id,
                    scope_type: release.scope_type,
                    scope_id: release.scope_id,
                    release_type: release.release_type,
                    release_version: release.release_version,
                    status: aggregate_release_status(&release.ack_status),
                    reason: release.reason,
                    counts,
                    created_at: release.created_at,
                }
            })
            .collect::<Vec<_>>();
        overviews.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        Ok(overviews)
    }

    pub async fn latest_release_for_node(
        &self,
        node_id: Uuid,
    ) -> Result<LatestReleaseResponse, AppError> {
        let release_id = self
            .repo
            .latest_release_id_for_node(node_id)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| AppError::not_found("no release available for node"))?;
        let release = self
            .repo
            .release(release_id)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| AppError::not_found("release not found"))?;

        Ok(LatestReleaseResponse {
            release_id,
            release_version: release.release_version.clone(),
            config_hash: release.bundle.manifest.config_hash.clone(),
            download_url: format!(
                "/api/node/config/package/{}?node_id={node_id}",
                release.release_version
            ),
        })
    }

    pub async fn config_package_for_node(
        &self,
        node_id: Uuid,
        version: &str,
    ) -> Result<ConfigPackageResponse, AppError> {
        let release = self
            .repo
            .release_by_version(version)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| AppError::not_found("release version not found"))?;
        if !release.target_node_ids.contains(&node_id) {
            return Err(AppError::unauthorized(
                "node is not a target of this release",
            ));
        }

        let manifest = serde_json::to_value(&release.bundle.manifest)
            .map_err(|error| AppError::internal(error.to_string()))?;
        let certificates = serde_json::to_value(&release.bundle.certificates)
            .map_err(|error| AppError::internal(error.to_string()))?;

        Ok(ConfigPackageResponse {
            release_id: release.release_id,
            release_version: release.release_version,
            manifest,
            rendered_config: release.bundle.rendered_config,
            certificates,
            signature: release.bundle.signature,
        })
    }

    pub async fn ack_release(
        &self,
        release_id: Uuid,
        request: ReleaseAckRequest,
    ) -> Result<ReleaseAckResponse, AppError> {
        let release = self
            .repo
            .release(release_id)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| AppError::not_found("release not found"))?;
        if !release.target_node_ids.contains(&request.node_id) {
            return Err(AppError::unauthorized(
                "node is not a target of this release",
            ));
        }

        let ack = NodeAckRecord {
            apply_status: request.apply_status.clone(),
            current_version: Some(request.current_version.clone()),
            message: request.message.clone(),
            acked_at: Some(Utc::now()),
        };
        self.repo
            .update_release_ack(release_id, request.node_id, &ack)
            .await
            .map_err(map_store_error)?;

        if let Some(mut node) = self
            .repo
            .node(request.node_id)
            .await
            .map_err(map_store_error)?
        {
            node.runtime.active_config_version = Some(request.current_version);
            node.runtime.last_seen_at = Some(Utc::now());
            self.repo.save_node(&node).await.map_err(map_store_error)?;
        }

        Ok(ReleaseAckResponse {
            release_id,
            node_id: request.node_id,
            apply_status: request.apply_status,
        })
    }

    pub async fn create_dns_provider(
        &self,
        request: CreateDnsProviderRequest,
    ) -> Result<CreateDnsProviderResponse, AppError> {
        validate_dns_provider_type(&request.provider_type)?;
        if self
            .repo
            .dns_provider_by_name(&request.name)
            .await
            .map_err(map_store_error)?
            .is_some()
        {
            return Err(AppError::conflict("dns provider name already exists"));
        }

        let provider = DnsProviderRecord {
            provider_id: Uuid::new_v4(),
            name: request.name,
            provider_type: request.provider_type,
            api_endpoint: request.api_endpoint,
            credential_encrypted: request.credentials.to_string(),
            status: "active".to_string(),
            created_at: Utc::now(),
        };
        self.repo
            .save_dns_provider(&provider)
            .await
            .map_err(map_store_error)?;

        Ok(CreateDnsProviderResponse {
            provider_id: provider.provider_id,
            name: provider.name,
            status: provider.status,
        })
    }

    pub async fn update_dns_provider(
        &self,
        provider_id: Uuid,
        request: CreateDnsProviderRequest,
    ) -> Result<CreateDnsProviderResponse, AppError> {
        validate_dns_provider_type(&request.provider_type)?;
        let existing = self
            .repo
            .dns_provider(provider_id)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| AppError::not_found("dns provider not found"))?;
        let duplicated = self
            .repo
            .dns_provider_by_name(&request.name)
            .await
            .map_err(map_store_error)?;
        if duplicated
            .as_ref()
            .map(|provider| provider.provider_id != provider_id)
            .unwrap_or(false)
        {
            return Err(AppError::conflict("dns provider name already exists"));
        }

        let provider = DnsProviderRecord {
            provider_id,
            name: request.name,
            provider_type: request.provider_type,
            api_endpoint: request.api_endpoint,
            credential_encrypted: request.credentials.to_string(),
            status: existing.status,
            created_at: existing.created_at,
        };
        self.repo
            .save_dns_provider(&provider)
            .await
            .map_err(map_store_error)?;

        Ok(CreateDnsProviderResponse {
            provider_id,
            name: provider.name,
            status: provider.status,
        })
    }

    pub async fn delete_dns_provider(
        &self,
        provider_id: Uuid,
    ) -> Result<DeleteDnsProviderResponse, AppError> {
        let provider = self
            .repo
            .dns_provider(provider_id)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| AppError::not_found("dns provider not found"))?;

        let has_zone = self
            .repo
            .list_dns_zones()
            .await
            .map_err(map_store_error)?
            .into_iter()
            .any(|zone| zone.provider_id == provider_id);
        if has_zone {
            return Err(AppError::conflict(
                "dns provider still owns synced zones; delete or disable those zones first",
            ));
        }

        self.repo
            .delete_dns_provider(provider_id)
            .await
            .map_err(map_store_error)?;
        Ok(DeleteDnsProviderResponse {
            provider_id,
            name: provider.name,
            deleted: true,
        })
    }

    pub async fn list_dns_providers(&self) -> Result<Vec<DnsProviderOverview>, AppError> {
        let mut providers = self
            .repo
            .list_dns_providers()
            .await
            .map_err(map_store_error)?
            .into_iter()
            .map(|provider| DnsProviderOverview {
                provider_id: provider.provider_id,
                name: provider.name,
                provider_type: provider.provider_type,
                api_endpoint: provider.api_endpoint,
                status: provider.status,
                created_at: provider.created_at,
            })
            .collect::<Vec<_>>();
        providers.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(providers)
    }

    pub async fn sync_dns_zones(
        &self,
        request: SyncDnsZonesRequest,
    ) -> Result<SyncDnsZonesResponse, AppError> {
        if request.zone_names.is_empty() {
            return Err(AppError::bad_request("zone_names must not be empty"));
        }

        let provider = self
            .repo
            .dns_provider(request.provider_id)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| AppError::not_found("dns provider not found"))?;
        let provider_config = provider_config_from_record(&provider)?;
        let zone_infos = build_provider(&provider_config)
            .map_err(|error| AppError::bad_request(error.to_string()))?
            .list_zones(&request.zone_names)
            .await
            .map_err(|error| AppError::bad_request(error.to_string()))?;
        if zone_infos.is_empty() {
            return Err(AppError::not_found("no dns zones found from provider"));
        }

        let mut zone_ids = Vec::with_capacity(zone_infos.len());
        for zone_info in zone_infos {
            let existing_zone = self
                .repo
                .dns_zone_by_provider_name(request.provider_id, &zone_info.zone_name)
                .await
                .map_err(map_store_error)?;
            let zone = DnsZoneRecord {
                zone_id: existing_zone
                    .map(|existing| existing.zone_id)
                    .unwrap_or_else(Uuid::new_v4),
                provider_id: request.provider_id,
                zone_name: zone_info.zone_name,
                external_zone_id: zone_info.zone_id,
                status: zone_info.status,
                created_at: Utc::now(),
            };
            zone_ids.push(zone.zone_id);
            self.repo
                .save_dns_zone(&zone)
                .await
                .map_err(map_store_error)?;
        }

        Ok(SyncDnsZonesResponse {
            provider_id: request.provider_id,
            zone_ids,
        })
    }

    pub async fn list_dns_zones(&self) -> Result<Vec<DnsZoneOverview>, AppError> {
        let mut zones = self
            .repo
            .list_dns_zones()
            .await
            .map_err(map_store_error)?
            .into_iter()
            .map(|zone| DnsZoneOverview {
                zone_id: zone.zone_id,
                provider_id: zone.provider_id,
                zone_name: zone.zone_name,
                status: zone.status,
                created_at: zone.created_at,
            })
            .collect::<Vec<_>>();
        zones.sort_by(|left, right| left.zone_name.cmp(&right.zone_name));
        Ok(zones)
    }

    pub async fn update_dns_zone_status(
        &self,
        zone_id: Uuid,
        enabled: bool,
    ) -> Result<UpdateDnsZoneStatusResponse, AppError> {
        let mut zone = self
            .repo
            .dns_zone(zone_id)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| AppError::not_found("dns zone not found"))?;
        zone.status = if enabled {
            "active".to_string()
        } else {
            "disabled".to_string()
        };
        self.repo
            .save_dns_zone(&zone)
            .await
            .map_err(map_store_error)?;
        Ok(UpdateDnsZoneStatusResponse {
            zone_id,
            zone_name: zone.zone_name,
            status: zone.status,
        })
    }

    pub async fn delete_dns_zone(&self, zone_id: Uuid) -> Result<DeleteDnsZoneResponse, AppError> {
        let zone = self
            .repo
            .dns_zone(zone_id)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| AppError::not_found("dns zone not found"))?;

        let has_orders = self
            .repo
            .list_certificate_orders()
            .await
            .map_err(map_store_error)?
            .into_iter()
            .any(|order| order.zone_id == zone_id);
        if has_orders {
            return Err(AppError::conflict(
                "dns zone is still referenced by certificate orders",
            ));
        }

        self.repo
            .delete_dns_zone(zone_id)
            .await
            .map_err(map_store_error)?;
        Ok(DeleteDnsZoneResponse {
            zone_id,
            zone_name: zone.zone_name,
            deleted: true,
        })
    }

    pub async fn create_certificate_order(
        &self,
        request: CreateCertificateOrderRequest,
    ) -> Result<CreateCertificateOrderResponse, AppError> {
        if request.challenge_type != "dns-01" {
            return Err(AppError::bad_request(
                "only dns-01 challenge is implemented",
            ));
        }

        let site = if let Some(site_id) = request.site_id {
            Some(
                self.repo
                    .site(site_id)
                    .await
                    .map_err(map_store_error)?
                    .ok_or_else(|| AppError::not_found("site not found"))?,
            )
        } else {
            None
        };
        let requested_domain = request
            .domain
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .or_else(|| site.as_ref().map(|site| site.domain.clone()))
            .ok_or_else(|| {
                AppError::bad_request("domain is required for standalone certificate orders")
            })?;
        let common_name = normalize_certificate_identifier(&requested_domain)?;
        let sans = normalize_certificate_sans(&common_name, &request.sans)?;
        if sans.len() > 1 {
            return Err(AppError::bad_request(
                "real acme certificate orders currently support a single dns identifier",
            ));
        }
        let zone = self
            .repo
            .dns_zone(request.zone_id)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| AppError::not_found("dns zone not found"))?;
        if zone.status != "active" {
            return Err(AppError::bad_request("dns zone must be active"));
        }
        if !domain_matches_zone(&common_name, &zone.zone_name) {
            return Err(AppError::bad_request(
                "certificate domain is not covered by selected dns zone",
            ));
        }
        self.issue_certificate_order(
            site.as_ref().map(|site| site.id),
            None,
            common_name,
            sans,
            request.zone_id,
            &zone.zone_name,
            request.acme_provider,
            "issue",
        )
        .await
    }

    pub async fn list_certificate_orders(&self) -> Result<Vec<CertificateOrderOverview>, AppError> {
        let mut orders = self
            .repo
            .list_certificate_orders()
            .await
            .map_err(map_store_error)?
            .into_iter()
            .map(|order| self.certificate_order_overview(order))
            .collect::<Vec<_>>();
        orders.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        Ok(orders)
    }

    pub async fn retry_certificate_order(
        &self,
        order_id: Uuid,
    ) -> Result<CertificateOrderOverview, AppError> {
        let order = self
            .repo
            .certificate_order(order_id)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| AppError::not_found("certificate order not found"))?;
        if !is_failed_certificate_order_status(&order.order_status) {
            return Err(AppError::conflict(
                "only failed certificate orders can be retried",
            ));
        }

        self.requeue_certificate_order(order, "retry").await
    }

    pub async fn reset_certificate_order(
        &self,
        order_id: Uuid,
    ) -> Result<CertificateOrderOverview, AppError> {
        let order = self
            .repo
            .certificate_order(order_id)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| AppError::not_found("certificate order not found"))?;
        if order.order_status == "issued" {
            return Err(AppError::conflict(
                "issued certificate orders cannot be reset",
            ));
        }

        self.requeue_certificate_order(order, "reset").await
    }

    async fn issue_node_tokens(&self, node_id: Uuid) -> Result<IssuedTokens, AppError> {
        self.tokens
            .issue_tokens(node_id, Duration::hours(24))
            .await
            .map_err(map_store_error)
    }

    async fn requeue_certificate_order(
        &self,
        mut order: CertificateOrderRecord,
        action: &str,
    ) -> Result<CertificateOrderOverview, AppError> {
        if order.order_status == "issued" {
            return Err(AppError::conflict(format!(
                "cannot {action} an issued certificate order"
            )));
        }

        order.order_status = "pending_dns_challenge".to_string();
        order.challenge_payload = json!({});
        order.error_message = None;
        self.repo
            .save_certificate_order(&order)
            .await
            .map_err(map_store_error)?;
        Ok(self.certificate_order_overview(order))
    }

    async fn issue_certificate_order(
        &self,
        site_id: Option<Uuid>,
        certificate_id: Option<Uuid>,
        common_name: String,
        sans: Vec<String>,
        zone_id: Uuid,
        zone_name: &str,
        acme_provider: String,
        order_type: &str,
    ) -> Result<CreateCertificateOrderResponse, AppError> {
        let certificate_id = match certificate_id {
            Some(certificate_id) => certificate_id,
            None => {
                let certificate = CertificateRecord {
                    certificate_id: Uuid::new_v4(),
                    cert_code: format!("cert-{}", Uuid::new_v4()),
                    common_name: common_name.clone(),
                    sans: sans.clone(),
                    fingerprint_sha256: format!("stub-{}", Uuid::new_v4()),
                    status: "staging".to_string(),
                    expires_at: None,
                    created_at: Utc::now(),
                };
                self.repo
                    .save_certificate(&certificate)
                    .await
                    .map_err(map_store_error)?;
                certificate.certificate_id
            }
        };

        let challenge_payload = json!({
            "zone_name": zone_name,
            "identifier": common_name,
            "sans": sans,
            "record_name": dns01_record_name(&common_name),
            "record_type": "TXT",
            "record_value": format!("token-{}", Uuid::new_v4()),
            "ttl": 60,
        });
        let order = CertificateOrderRecord {
            order_id: Uuid::new_v4(),
            site_id,
            certificate_id: Some(certificate_id),
            zone_id,
            order_type: order_type.to_string(),
            acme_provider,
            challenge_type: "dns-01".to_string(),
            challenge_payload: challenge_payload.clone(),
            order_status: "pending_dns_challenge".to_string(),
            error_message: None,
            certificate_expires_at: None,
            created_at: Utc::now(),
        };
        self.repo
            .save_certificate_order(&order)
            .await
            .map_err(map_store_error)?;

        Ok(CreateCertificateOrderResponse {
            order_id: order.order_id,
            certificate_id: order.certificate_id,
            order_status: order.order_status,
            challenge_payload,
        })
    }

    fn certificate_order_overview(
        &self,
        order: CertificateOrderRecord,
    ) -> CertificateOrderOverview {
        certificate_order_overview(order, self.cert_auto_renew_before_days)
    }
}

fn parse_protocol(raw: &str) -> Result<SiteProtocol, AppError> {
    match raw.to_ascii_lowercase().as_str() {
        "http" => Ok(SiteProtocol::Http),
        "https" => Ok(SiteProtocol::Https),
        "tcp" => Ok(SiteProtocol::Tcp),
        _ => Err(AppError::bad_request("unsupported protocol")),
    }
}

fn summarize_release_counts(ack_status: &HashMap<Uuid, NodeAckRecord>) -> ReleaseStatusCounts {
    let mut counts = ReleaseStatusCounts::default();
    for ack in ack_status.values() {
        match ack.apply_status.as_str() {
            "success" => counts.success += 1,
            "failed" => counts.failed += 1,
            "downloading" | "applying" => counts.in_progress += 1,
            _ => counts.pending += 1,
        }
    }
    counts
}

fn normalize_certificate_identifier(raw: &str) -> Result<String, AppError> {
    let value = raw.trim().trim_end_matches('.').to_ascii_lowercase();
    if value.is_empty() {
        return Err(AppError::bad_request("certificate domain is required"));
    }
    if value.contains("://") || value.contains('/') || value.contains(':') {
        return Err(AppError::bad_request(
            "certificate domain must be a dns name",
        ));
    }
    if value.contains('*') && !value.starts_with("*.") {
        return Err(AppError::bad_request(
            "wildcard certificate must use the left-most *. label",
        ));
    }

    let base = certificate_dns_domain(&value);
    if !base.contains('.') {
        return Err(AppError::bad_request(
            "certificate domain must include a zone",
        ));
    }
    for label in base.split('.') {
        let valid = !label.is_empty()
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-');
        if !valid {
            return Err(AppError::bad_request(
                "certificate domain contains an invalid label",
            ));
        }
    }

    Ok(value)
}

fn normalize_certificate_sans(
    common_name: &str,
    raw_sans: &[String],
) -> Result<Vec<String>, AppError> {
    let mut seen = HashSet::<String>::new();
    let mut sans = Vec::new();
    for value in std::iter::once(common_name.to_string()).chain(raw_sans.iter().cloned()) {
        let identifier = normalize_certificate_identifier(&value)?;
        if seen.insert(identifier.clone()) {
            sans.push(identifier);
        }
    }
    Ok(sans)
}

fn certificate_dns_domain(identifier: &str) -> &str {
    identifier.strip_prefix("*.").unwrap_or(identifier)
}

fn dns01_record_name(identifier: &str) -> String {
    format!("_acme-challenge.{}", certificate_dns_domain(identifier))
}

fn domain_matches_zone(domain: &str, zone_name: &str) -> bool {
    let domain = certificate_dns_domain(domain)
        .trim_end_matches('.')
        .to_ascii_lowercase();
    let zone_name = zone_name.trim().trim_end_matches('.').to_ascii_lowercase();
    domain == zone_name || domain.ends_with(&format!(".{zone_name}"))
}

fn validate_dns_provider_type(provider_type: &str) -> Result<(), AppError> {
    match provider_type {
        "cloudflare" | "alidns" | "route53" | "dnspod" | "custom" | "noop" => Ok(()),
        _ => Err(AppError::bad_request("unsupported dns provider type")),
    }
}

fn certificate_order_overview(
    order: CertificateOrderRecord,
    cert_auto_renew_before_days: i64,
) -> CertificateOrderOverview {
    let certificate_expires_at = order.certificate_expires_at;
    let next_renew_at =
        next_certificate_renew_at(certificate_expires_at, cert_auto_renew_before_days);
    CertificateOrderOverview {
        order_id: order.order_id,
        site_id: order.site_id,
        certificate_id: order.certificate_id,
        zone_id: order.zone_id,
        order_type: order.order_type,
        acme_provider: order.acme_provider,
        challenge_type: order.challenge_type,
        order_status: order.order_status,
        challenge_payload: order.challenge_payload,
        error_message: order.error_message,
        certificate_expires_at,
        next_renew_at,
        created_at: order.created_at,
    }
}

fn next_certificate_renew_at(
    expires_at: Option<DateTime<Utc>>,
    cert_auto_renew_before_days: i64,
) -> Option<DateTime<Utc>> {
    expires_at.map(|value| {
        value
            - Duration::days(normalize_cert_auto_renew_before_days(
                cert_auto_renew_before_days,
            ))
    })
}

fn is_failed_certificate_order_status(status: &str) -> bool {
    matches!(status, "dns_challenge_failed" | "issue_failed")
}

fn normalize_cert_auto_renew_before_days(value: i64) -> i64 {
    value.max(1)
}

fn provider_config_from_record(record: &DnsProviderRecord) -> Result<ProviderConfig, AppError> {
    let credentials = serde_json::from_str(&record.credential_encrypted)
        .map_err(|error| AppError::internal(error.to_string()))?;
    Ok(ProviderConfig {
        provider_type: record.provider_type.clone(),
        api_endpoint: record.api_endpoint.clone(),
        credentials,
    })
}

fn normalize_admin_username(raw: &str) -> Result<String, AppError> {
    let normalized = raw.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return Err(AppError::bad_request("username is required"));
    }
    Ok(normalized)
}

fn normalize_admin_display_name(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        "Administrator".to_string()
    } else {
        trimmed.to_string()
    }
}

async fn generate_unique_site_code(
    repo: &Arc<dyn HubRepository>,
    preferred: Option<&str>,
    name: &str,
    domain: &str,
) -> Result<String, AppError> {
    let base = preferred
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(slugify_site_code)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            let domain_part = domain.split('.').next().unwrap_or(domain);
            let generated = slugify_site_code(domain_part);
            (!generated.is_empty()).then_some(generated)
        })
        .or_else(|| {
            let generated = slugify_site_code(name);
            (!generated.is_empty()).then_some(generated)
        })
        .unwrap_or_else(|| format!("site-{}", Uuid::new_v4().simple()));

    let mut candidate = base.clone();
    let mut suffix = 1_u32;
    while repo
        .site_id_by_code(&candidate)
        .await
        .map_err(map_store_error)?
        .is_some()
    {
        suffix += 1;
        candidate = format!("{base}-{suffix}");
    }
    Ok(candidate)
}

fn slugify_site_code(raw: &str) -> String {
    let mut value = String::new();
    let mut previous_dash = false;
    for ch in raw.chars() {
        let normalized = match ch {
            'a'..='z' | '0'..='9' => Some(ch),
            'A'..='Z' => Some(ch.to_ascii_lowercase()),
            '-' | '_' | '.' | ' ' => Some('-'),
            _ => None,
        };
        let Some(ch) = normalized else {
            continue;
        };
        if ch == '-' {
            if previous_dash || value.is_empty() {
                continue;
            }
            previous_dash = true;
            value.push(ch);
        } else {
            previous_dash = false;
            value.push(ch);
        }
    }
    value.trim_matches('-').to_string()
}

fn hash_password(raw_password: &str) -> Result<String, AppError> {
    if raw_password.len() < 6 {
        return Err(AppError::bad_request(
            "password must contain at least 6 characters",
        ));
    }
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(raw_password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| AppError::internal(error.to_string()))
}

fn verify_password(password_hash: &str, raw_password: &str) -> Result<(), AppError> {
    let parsed_hash =
        PasswordHash::new(password_hash).map_err(|error| AppError::internal(error.to_string()))?;
    Argon2::default()
        .verify_password(raw_password.as_bytes(), &parsed_hash)
        .map_err(|_| AppError::unauthorized("invalid username or password"))
}

fn generate_admin_session_token() -> String {
    format!("adm_{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

fn hash_session_token(raw_token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(raw_token.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn hash_bootstrap_token(raw_token: &str) -> String {
    hash_session_token(raw_token)
}

fn build_release_ack_status(target_node_ids: &[Uuid]) -> HashMap<Uuid, NodeAckRecord> {
    let mut ack_status = HashMap::new();
    for node_id in target_node_ids {
        ack_status.insert(
            *node_id,
            NodeAckRecord {
                apply_status: "pending".to_string(),
                current_version: None,
                message: None,
                acked_at: None,
            },
        );
    }
    ack_status
}

impl SavedRelease {
    fn into_response(self) -> CreateReleaseResponse {
        CreateReleaseResponse {
            release_id: self.release_id,
            release_version: self.release_version,
            config_hash: self.config_hash,
            target_node_ids: self.target_node_ids,
        }
    }
}

fn normalize_operation_template_name(raw: &str) -> Result<String, AppError> {
    let normalized = raw.trim().to_lowercase();
    if normalized.is_empty() {
        return Err(AppError::bad_request("operation template name is required"));
    }
    Ok(normalized)
}

fn normalize_operation_type(raw: &str) -> Result<String, AppError> {
    let normalized = raw.trim().to_lowercase();
    if matches!(
        normalized.as_str(),
        "vip_bind" | "vip_unbind" | "route_switch" | "service_reload" | "custom_template"
    ) {
        Ok(normalized)
    } else {
        Err(AppError::bad_request(
            "operation_type must be one of vip_bind, vip_unbind, route_switch, service_reload, custom_template",
        ))
    }
}

fn normalize_allowed_params(raw: &[String]) -> Result<Vec<String>, AppError> {
    let mut params = raw
        .iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>();
    params.sort();
    params.dedup();
    if params.len() > 32 {
        return Err(AppError::bad_request("allowed_params is too large"));
    }
    Ok(params)
}

fn normalize_operation_run_as_user(raw: &str) -> String {
    let normalized = raw.trim();
    if normalized.is_empty() {
        "root".to_string()
    } else {
        normalized.to_string()
    }
}

fn primary_site_binding(site: &SiteRecord) -> Option<&SiteBindingRecord> {
    site.bindings
        .iter()
        .find(|binding| binding.binding_role == "primary")
        .or_else(|| site.bindings.first())
}

fn site_binding_item(
    binding: SiteBindingRecord,
    node_lookup: &HashMap<Uuid, NodeRecord>,
) -> SiteBindingItem {
    let node = node_lookup.get(&binding.node_id);
    SiteBindingItem {
        node_id: binding.node_id,
        binding_role: binding.binding_role,
        priority: binding.priority,
        node_code: node.map(|node| node.identity.node_code.clone()),
        node_name: node.map(|node| node.identity.name.clone()),
        node_status: node.map(|node| node_status_name(node.identity.status)),
    }
}

fn normalize_site_bindings(raw: Vec<SiteBindingItem>) -> Result<Vec<SiteBindingRecord>, AppError> {
    if raw.is_empty() {
        return Err(AppError::bad_request("at least one binding is required"));
    }

    let mut bindings = Vec::with_capacity(raw.len());
    let mut seen_node_ids = HashSet::new();
    let mut primary_count = 0;
    for binding in raw {
        if !seen_node_ids.insert(binding.node_id) {
            return Err(AppError::bad_request(format!(
                "duplicate binding for node {}",
                binding.node_id
            )));
        }

        let binding_role = normalize_binding_role(&binding.binding_role)?;
        if binding_role == "primary" {
            primary_count += 1;
        }
        bindings.push(SiteBindingRecord {
            node_id: binding.node_id,
            binding_role,
            priority: binding.priority,
        });
    }

    if primary_count != 1 {
        return Err(AppError::bad_request(
            "exactly one primary binding is required",
        ));
    }

    bindings.sort_by_key(|binding| binding.priority);
    Ok(bindings)
}

fn normalize_binding_role(raw: &str) -> Result<String, AppError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "primary" => Ok("primary".to_string()),
        "standby" => Ok("standby".to_string()),
        _ => Err(AppError::bad_request(
            "binding_role must be one of primary, standby",
        )),
    }
}

fn normalize_operation_input_params(raw: &Value) -> Result<Map<String, Value>, AppError> {
    match raw {
        Value::Object(map) => Ok(map.clone()),
        _ => Err(AppError::bad_request("input_params must be a JSON object")),
    }
}

fn build_operation_template(
    record: &OperationTemplateRecord,
) -> Result<OperationTemplate, AppError> {
    let kind = match record.operation_type.as_str() {
        "vip_bind" => OperationKind::VipBind,
        "vip_unbind" => OperationKind::VipUnbind,
        "route_switch" => OperationKind::RouteSwitch,
        "service_reload" => OperationKind::ServiceReload,
        "custom_template" => OperationKind::CustomTemplate,
        _ => {
            return Err(AppError::internal(format!(
                "unsupported operation type {}",
                record.operation_type
            )));
        }
    };

    let allowed_params = record
        .allowed_params
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(OperationTemplate {
        name: record.name.clone(),
        kind,
        command_template: record.command_template.clone(),
        allowed_params,
        timeout_seconds: u64::from(record.timeout_seconds.max(1)),
    })
}

fn operation_template_overview(
    template: &OperationTemplateRecord,
) -> Result<OperationTemplateOverview, AppError> {
    Ok(OperationTemplateOverview {
        template_id: template.template_id,
        name: template.name.clone(),
        operation_type: template.operation_type.clone(),
        command_template: template.command_template.clone(),
        allowed_params: normalize_operation_input_array(&template.allowed_params),
        timeout_seconds: template.timeout_seconds,
        run_as_user: template.run_as_user.clone(),
        approval_required: template.approval_required,
        created_at: template.created_at,
    })
}

fn node_operation_overview(
    operation: &NodeOperationRecord,
    template: &OperationTemplateRecord,
) -> Result<NodeOperationOverview, AppError> {
    Ok(NodeOperationOverview {
        operation_id: operation.operation_id,
        node_id: operation.node_id,
        template_id: template.template_id,
        template_name: template.name.clone(),
        operation_type: template.operation_type.clone(),
        input_params: operation.input_params.clone(),
        exec_status: operation.exec_status.clone(),
        requested_by: operation.requested_by.clone(),
        approved_by: operation.approved_by.clone(),
        exit_code: operation.exit_code,
        stdout_log: operation.stdout_log.clone(),
        stderr_log: operation.stderr_log.clone(),
        started_at: operation.started_at,
        finished_at: operation.finished_at,
        created_at: operation.created_at,
    })
}

fn claimed_node_operation_dispatch(
    claimed: ClaimedNodeOperation,
) -> Result<PendingOperationResponse, AppError> {
    let template = build_operation_template(&claimed.template)?;
    let params = normalize_operation_input_params(&claimed.operation.input_params)?;
    let rendered_command = template
        .render_command(&params)
        .map_err(|error: OperationValidationError| AppError::bad_request(error.to_string()))?;

    Ok(PendingOperationResponse {
        operation_id: claimed.operation.operation_id,
        node_id: claimed.operation.node_id,
        template_id: claimed.template.template_id,
        template_name: claimed.template.name,
        operation_type: claimed.template.operation_type,
        rendered_command,
        timeout_seconds: template.timeout_seconds,
        run_as_user: claimed.template.run_as_user,
        created_at: claimed.operation.created_at,
    })
}

fn normalize_operation_input_array(raw: &Value) -> Vec<String> {
    raw.as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn normalize_operation_exec_status(raw: &str) -> Result<String, AppError> {
    let normalized = raw.trim().to_lowercase();
    if matches!(
        normalized.as_str(),
        "success" | "failed" | "timeout" | "cancelled"
    ) {
        Ok(normalized)
    } else {
        Err(AppError::bad_request(
            "exec_status must be one of success, failed, timeout, cancelled",
        ))
    }
}

fn truncate_log_output(raw: &str) -> String {
    const MAX_LEN: usize = 4_000;
    if raw.chars().count() <= MAX_LEN {
        raw.to_string()
    } else {
        let truncated = raw.chars().take(MAX_LEN).collect::<String>();
        format!("{truncated}...")
    }
}

fn validate_admin_session(session: &AdminSessionContext) -> Result<(), AppError> {
    if session.status != "active" {
        return Err(AppError::unauthorized("admin account is disabled"));
    }
    if session.expires_at < Utc::now() {
        return Err(AppError::unauthorized("admin session expired"));
    }
    Ok(())
}

fn map_store_error(error: StoreError) -> AppError {
    AppError::internal(error.to_string())
}

fn is_foreign_key_violation(error: &StoreError) -> bool {
    match error {
        StoreError::Sqlx(sqlx::Error::Database(database_error)) => {
            database_error.code().as_deref() == Some("23503")
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{store::MemoryRepository, token_store::MemoryTokenStore};
    use serde_json::json;
    use std::sync::{Arc, atomic::AtomicU64};

    fn sample_certificate_order(
        status: &str,
        error_message: Option<&str>,
    ) -> CertificateOrderRecord {
        CertificateOrderRecord {
            order_id: Uuid::new_v4(),
            site_id: Some(Uuid::new_v4()),
            certificate_id: Some(Uuid::new_v4()),
            zone_id: Uuid::new_v4(),
            order_type: "issue".to_string(),
            acme_provider: "letsencrypt".to_string(),
            challenge_type: "dns-01".to_string(),
            challenge_payload: json!({
                "record_name": "_acme-challenge.demo.example.com",
                "record_value": "token-value",
                "challenge_url": "https://example.invalid/challenge"
            }),
            order_status: status.to_string(),
            error_message: error_message.map(ToString::to_string),
            certificate_expires_at: None,
            created_at: Utc::now(),
        }
    }

    fn sample_node_record(code: &str, name: &str, status: NodeStatus) -> NodeRecord {
        NodeRecord {
            identity: NodeIdentity {
                id: Uuid::new_v4(),
                node_code: code.to_string(),
                name: name.to_string(),
                region: "cn-east".to_string(),
                idc: "lab".to_string(),
                status,
            },
            runtime: NodeRuntimeState {
                active_config_version: None,
                last_seen_at: Some(Utc::now()),
                site_count: 0,
                health_score: 100,
            },
            labels: json!({}),
            hostname: Some(format!("{code}.local")),
            public_ip: None,
            private_ip: None,
            bootstrap_token: None,
            pingora_version: Some("0.5.0".to_string()),
            agent_version: Some("0.1.0".to_string()),
        }
    }

    #[tokio::test]
    async fn site_overview_and_detail_include_binding_node_names() {
        let repo = Arc::new(MemoryRepository::default());
        let service = HubService {
            repo: repo.clone(),
            tokens: Arc::new(MemoryTokenStore::default()),
            release_counter: Arc::new(AtomicU64::new(1)),
            cert_auto_renew_before_days: CERT_AUTO_RENEW_BEFORE_DAYS,
        };

        let primary = sample_node_record("edge-a", "Edge A", NodeStatus::Online);
        let standby = sample_node_record("edge-b", "Edge B", NodeStatus::Suspect);
        repo.save_node(&primary).await.unwrap();
        repo.save_node(&standby).await.unwrap();

        let site = SiteRecord {
            id: Uuid::new_v4(),
            site_code: "friendly-site".to_string(),
            name: "Friendly Site".to_string(),
            domain: "friendly.example.com".to_string(),
            listen_port: 443,
            protocol: SiteProtocol::Https,
            tls_enabled: true,
            status: SiteStatus::Draft,
            version: 1,
            config: json!({ "upstreams": [] }),
            bindings: vec![
                SiteBindingRecord {
                    node_id: primary.identity.id,
                    binding_role: "primary".to_string(),
                    priority: 10,
                },
                SiteBindingRecord {
                    node_id: standby.identity.id,
                    binding_role: "standby".to_string(),
                    priority: 20,
                },
            ],
        };
        repo.save_site(&site).await.unwrap();

        let overview = service
            .list_sites()
            .await
            .unwrap()
            .into_iter()
            .find(|item| item.site_id == site.id)
            .unwrap();
        assert_eq!(overview.primary_node_id, Some(primary.identity.id));
        assert_eq!(overview.primary_node_code.as_deref(), Some("edge-a"));
        assert_eq!(overview.primary_node_name.as_deref(), Some("Edge A"));
        assert_eq!(overview.primary_node_status.as_deref(), Some("online"));

        let detail = service.site_detail(site.id).await.unwrap();
        let standby_binding = detail
            .bindings
            .iter()
            .find(|binding| binding.node_id == standby.identity.id)
            .unwrap();
        assert_eq!(standby_binding.node_code.as_deref(), Some("edge-b"));
        assert_eq!(standby_binding.node_name.as_deref(), Some("Edge B"));
        assert_eq!(standby_binding.node_status.as_deref(), Some("suspect"));
    }

    #[tokio::test]
    async fn retry_certificate_order_requeues_failed_orders() {
        let service = HubService::new_memory();
        let order = sample_certificate_order("dns_challenge_failed", Some("boom"));
        service.repo.save_certificate_order(&order).await.unwrap();

        let response = service
            .retry_certificate_order(order.order_id)
            .await
            .unwrap();

        assert_eq!(response.order_status, "pending_dns_challenge");
        assert_eq!(response.challenge_payload, json!({}));
        assert_eq!(response.error_message, None);
    }

    #[tokio::test]
    async fn reset_certificate_order_rejects_issued_orders() {
        let service = HubService::new_memory();
        let order = sample_certificate_order("issued", None);
        service.repo.save_certificate_order(&order).await.unwrap();

        let error = service
            .reset_certificate_order(order.order_id)
            .await
            .unwrap_err();

        assert_eq!(error.status, axum::http::StatusCode::CONFLICT);
    }

    #[test]
    fn certificate_order_overview_includes_next_renew_time() {
        let expires_at = Utc::now() + Duration::days(90);
        let mut order = sample_certificate_order("issued", None);
        order.certificate_expires_at = Some(expires_at);

        let overview = certificate_order_overview(order, CERT_AUTO_RENEW_BEFORE_DAYS);

        assert_eq!(overview.certificate_expires_at, Some(expires_at));
        assert_eq!(
            overview.next_renew_at,
            Some(expires_at - Duration::days(CERT_AUTO_RENEW_BEFORE_DAYS))
        );
    }

    #[test]
    fn certificate_order_overview_uses_service_configured_renew_before_days() {
        let service = HubService::new_memory_with_cert_auto_renew_before_days(14);
        let expires_at = Utc::now() + Duration::days(90);
        let mut order = sample_certificate_order("issued", None);
        order.certificate_expires_at = Some(expires_at);

        let overview = service.certificate_order_overview(order);

        assert_eq!(
            overview.next_renew_at,
            Some(expires_at - Duration::days(14))
        );
    }

    #[tokio::test]
    async fn create_certificate_order_supports_standalone_wildcard_domain() {
        let service = HubService::new_memory();
        let zone = DnsZoneRecord {
            zone_id: Uuid::new_v4(),
            provider_id: Uuid::new_v4(),
            zone_name: "example.com".to_string(),
            external_zone_id: "zone-example".to_string(),
            status: "active".to_string(),
            created_at: Utc::now(),
        };
        service.repo.save_dns_zone(&zone).await.unwrap();

        let response = service
            .create_certificate_order(CreateCertificateOrderRequest {
                site_id: None,
                domain: Some("*.example.com".to_string()),
                sans: vec![],
                acme_provider: "mock".to_string(),
                challenge_type: "dns-01".to_string(),
                zone_id: zone.zone_id,
            })
            .await
            .unwrap();

        let order = service
            .repo
            .certificate_order(response.order_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(order.site_id, None);
        assert_eq!(order.challenge_payload["identifier"], "*.example.com");
        assert_eq!(
            order.challenge_payload["record_name"],
            "_acme-challenge.example.com"
        );
    }

    #[tokio::test]
    async fn create_certificate_order_rejects_multi_san_until_worker_supports_it() {
        let service = HubService::new_memory();
        let zone = DnsZoneRecord {
            zone_id: Uuid::new_v4(),
            provider_id: Uuid::new_v4(),
            zone_name: "example.com".to_string(),
            external_zone_id: "zone-example".to_string(),
            status: "active".to_string(),
            created_at: Utc::now(),
        };
        service.repo.save_dns_zone(&zone).await.unwrap();

        let error = service
            .create_certificate_order(CreateCertificateOrderRequest {
                site_id: None,
                domain: Some("api.example.com".to_string()),
                sans: vec!["www.example.com".to_string()],
                acme_provider: "letsencrypt".to_string(),
                challenge_type: "dns-01".to_string(),
                zone_id: zone.zone_id,
            })
            .await
            .unwrap_err();

        assert_eq!(error.status, axum::http::StatusCode::BAD_REQUEST);
        assert!(error.message.contains("single dns identifier"));
    }

    #[tokio::test]
    async fn renew_site_certificate_reuses_existing_certificate_binding() {
        let repo = Arc::new(MemoryRepository::default());
        let service = HubService {
            repo: repo.clone(),
            tokens: Arc::new(MemoryTokenStore::default()),
            release_counter: Arc::new(AtomicU64::new(1)),
            cert_auto_renew_before_days: CERT_AUTO_RENEW_BEFORE_DAYS,
        };
        let site = SiteRecord {
            id: Uuid::new_v4(),
            site_code: "tls-site".to_string(),
            name: "TLS Site".to_string(),
            domain: "www.example.com".to_string(),
            listen_port: 443,
            protocol: SiteProtocol::Https,
            tls_enabled: true,
            status: SiteStatus::Published,
            version: 1,
            config: json!({}),
            bindings: vec![],
        };
        let zone = DnsZoneRecord {
            zone_id: Uuid::new_v4(),
            provider_id: Uuid::new_v4(),
            zone_name: "example.com".to_string(),
            external_zone_id: "zone-example".to_string(),
            status: "active".to_string(),
            created_at: Utc::now(),
        };
        let certificate_id = Uuid::new_v4();
        service.repo.save_site(&site).await.unwrap();
        service.repo.save_dns_zone(&zone).await.unwrap();
        repo.set_site_certificates(
            site.id,
            vec![pingorahub_domain::CertificateRef {
                certificate_id,
                version: 3,
                common_name: site.domain.clone(),
                sans: vec![site.domain.clone()],
                expires_at: None,
                cert_pem: "cert".to_string(),
                key_pem_encrypted: "key".to_string(),
                chain_pem: None,
            }],
        )
        .await;

        let response = service.renew_site_certificate(site.id).await.unwrap();
        let order = service
            .repo
            .certificate_order(response.order_id)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(response.certificate_id, Some(certificate_id));
        assert_eq!(order.site_id, Some(site.id));
        assert_eq!(order.certificate_id, Some(certificate_id));
        assert_eq!(order.order_type, "renew");
    }

    #[tokio::test]
    async fn change_admin_password_updates_credentials_and_revokes_other_sessions() {
        let service = HubService::new_memory();
        service
            .ensure_admin_user("admin", "平台管理员", "Admin#2026!")
            .await
            .unwrap();

        let (_first_login, first_session_token) = service
            .login_admin(AdminLoginRequest {
                username: "admin".to_string(),
                password: "Admin#2026!".to_string(),
            })
            .await
            .unwrap();
        let (_second_login, second_session_token) = service
            .login_admin(AdminLoginRequest {
                username: "admin".to_string(),
                password: "Admin#2026!".to_string(),
            })
            .await
            .unwrap();

        let response = service
            .change_admin_password(
                &first_session_token,
                ChangeAdminPasswordRequest {
                    current_password: "Admin#2026!".to_string(),
                    new_password: "Admin#2026!Next".to_string(),
                },
            )
            .await
            .unwrap();
        assert!(response.changed);

        assert!(service.current_admin(&first_session_token).await.is_ok());
        let revoked_error = service
            .current_admin(&second_session_token)
            .await
            .unwrap_err();
        assert_eq!(revoked_error.status, axum::http::StatusCode::UNAUTHORIZED);

        let old_password_error = service
            .login_admin(AdminLoginRequest {
                username: "admin".to_string(),
                password: "Admin#2026!".to_string(),
            })
            .await
            .unwrap_err();
        assert_eq!(
            old_password_error.status,
            axum::http::StatusCode::UNAUTHORIZED
        );

        assert!(
            service
                .login_admin(AdminLoginRequest {
                    username: "admin".to_string(),
                    password: "Admin#2026!Next".to_string(),
                })
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn create_release_includes_default_site_certificates() {
        let repo = Arc::new(MemoryRepository::default());
        let service = HubService {
            repo: repo.clone(),
            tokens: Arc::new(MemoryTokenStore::default()),
            release_counter: Arc::new(AtomicU64::new(1)),
            cert_auto_renew_before_days: CERT_AUTO_RENEW_BEFORE_DAYS,
        };

        let node = NodeRecord {
            identity: NodeIdentity {
                id: Uuid::new_v4(),
                node_code: "cert-node".to_string(),
                name: "Certificate Node".to_string(),
                region: "cn-east".to_string(),
                idc: "lab".to_string(),
                status: NodeStatus::Online,
            },
            runtime: NodeRuntimeState {
                active_config_version: None,
                last_seen_at: Some(Utc::now()),
                site_count: 0,
                health_score: 100,
            },
            labels: json!({ "role": "edge" }),
            hostname: None,
            public_ip: None,
            private_ip: None,
            bootstrap_token: None,
            pingora_version: Some("0.5.0".to_string()),
            agent_version: Some("0.1.0".to_string()),
        };
        repo.save_node(&node).await.unwrap();

        let site = SiteRecord {
            id: Uuid::new_v4(),
            site_code: "cert-site".to_string(),
            name: "Certificate Site".to_string(),
            domain: "cert.example.com".to_string(),
            listen_port: 443,
            protocol: SiteProtocol::Https,
            tls_enabled: true,
            status: SiteStatus::Draft,
            version: 1,
            config: json!({ "upstreams": [] }),
            bindings: vec![SiteBindingRecord {
                node_id: node.identity.id,
                binding_role: "primary".to_string(),
                priority: 10,
            }],
        };
        repo.save_site(&site).await.unwrap();
        repo.set_site_certificates(
            site.id,
            vec![pingorahub_domain::CertificateRef {
                certificate_id: Uuid::new_v4(),
                version: 3,
                common_name: "cert.example.com".to_string(),
                sans: vec!["cert.example.com".to_string()],
                expires_at: Some(Utc::now() + chrono::Duration::days(60)),
                cert_pem: "-----BEGIN CERTIFICATE-----\nmock\n-----END CERTIFICATE-----".to_string(),
                key_pem_encrypted:
                    "-----BEGIN ENCRYPTED PRIVATE KEY-----\nmock\n-----END ENCRYPTED PRIVATE KEY-----"
                        .to_string(),
                chain_pem: Some(
                    "-----BEGIN CERTIFICATE-----\nchain\n-----END CERTIFICATE-----".to_string(),
                ),
            }],
        )
        .await;

        let release = service
            .create_release(CreateReleaseRequest {
                scope_type: "site".to_string(),
                scope_id: site.id,
                release_type: "publish".to_string(),
                reason: "certificate publish".to_string(),
            })
            .await
            .unwrap();

        let saved_release = service
            .repo
            .release(release.release_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved_release.bundle.certificates.len(), 1);
        assert_eq!(
            saved_release.bundle.certificates[0].common_name,
            "cert.example.com"
        );
        assert!(
            saved_release.bundle.certificates[0]
                .cert_pem
                .contains("BEGIN CERTIFICATE")
        );
    }

    #[tokio::test]
    async fn create_release_emits_cleanup_release_for_detached_nodes() {
        let repo = Arc::new(MemoryRepository::default());
        let service = HubService {
            repo: repo.clone(),
            tokens: Arc::new(MemoryTokenStore::default()),
            release_counter: Arc::new(AtomicU64::new(1)),
            cert_auto_renew_before_days: CERT_AUTO_RENEW_BEFORE_DAYS,
        };

        let old_node = NodeRecord {
            identity: NodeIdentity {
                id: Uuid::new_v4(),
                node_code: "old-node".to_string(),
                name: "Old Node".to_string(),
                region: "cn-east".to_string(),
                idc: "lab".to_string(),
                status: NodeStatus::Online,
            },
            runtime: NodeRuntimeState {
                active_config_version: None,
                last_seen_at: Some(Utc::now()),
                site_count: 0,
                health_score: 100,
            },
            labels: json!({}),
            hostname: Some("old-node.local".to_string()),
            public_ip: None,
            private_ip: None,
            bootstrap_token: None,
            pingora_version: Some("0.5.0".to_string()),
            agent_version: Some("0.1.0".to_string()),
        };
        repo.save_node(&old_node).await.unwrap();

        let new_node = NodeRecord {
            identity: NodeIdentity {
                id: Uuid::new_v4(),
                node_code: "new-node".to_string(),
                name: "New Node".to_string(),
                region: "cn-east".to_string(),
                idc: "lab".to_string(),
                status: NodeStatus::Online,
            },
            runtime: NodeRuntimeState {
                active_config_version: None,
                last_seen_at: Some(Utc::now()),
                site_count: 0,
                health_score: 100,
            },
            labels: json!({}),
            hostname: Some("new-node.local".to_string()),
            public_ip: None,
            private_ip: None,
            bootstrap_token: None,
            pingora_version: Some("0.5.0".to_string()),
            agent_version: Some("0.1.0".to_string()),
        };
        repo.save_node(&new_node).await.unwrap();

        let site = SiteRecord {
            id: Uuid::new_v4(),
            site_code: "cleanup-site".to_string(),
            name: "Cleanup Site".to_string(),
            domain: "cleanup.example.com".to_string(),
            listen_port: 443,
            protocol: SiteProtocol::Https,
            tls_enabled: true,
            status: SiteStatus::Draft,
            version: 1,
            config: json!({ "upstreams": [] }),
            bindings: vec![SiteBindingRecord {
                node_id: old_node.identity.id,
                binding_role: "primary".to_string(),
                priority: 10,
            }],
        };
        repo.save_site(&site).await.unwrap();

        let first_release = service
            .create_release(CreateReleaseRequest {
                scope_type: "site".to_string(),
                scope_id: site.id,
                release_type: "publish".to_string(),
                reason: "initial publish".to_string(),
            })
            .await
            .unwrap();
        assert_eq!(first_release.target_node_ids, vec![old_node.identity.id]);

        service
            .bind_site_nodes(
                site.id,
                CreateSiteBindingsRequest {
                    bindings: vec![SiteBindingItem {
                        node_id: new_node.identity.id,
                        binding_role: "primary".to_string(),
                        priority: 10,
                        node_code: None,
                        node_name: None,
                        node_status: None,
                    }],
                },
            )
            .await
            .unwrap();

        let second_release = service
            .create_release(CreateReleaseRequest {
                scope_type: "site".to_string(),
                scope_id: site.id,
                release_type: "publish".to_string(),
                reason: "move to new node".to_string(),
            })
            .await
            .unwrap();
        assert_eq!(second_release.target_node_ids, vec![new_node.identity.id]);

        let new_node_latest = service
            .latest_release_for_node(new_node.identity.id)
            .await
            .unwrap();
        assert_eq!(new_node_latest.release_id, second_release.release_id);

        let old_node_latest = service
            .latest_release_for_node(old_node.identity.id)
            .await
            .unwrap();
        assert_ne!(old_node_latest.release_id, first_release.release_id);
        assert_ne!(old_node_latest.release_id, second_release.release_id);

        let cleanup_package = service
            .config_package_for_node(old_node.identity.id, &old_node_latest.release_version)
            .await
            .unwrap();
        assert_eq!(
            cleanup_package
                .manifest
                .get("sites")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(0)
        );
        assert_eq!(
            cleanup_package.rendered_config["cleanup"]["site_codes"][0],
            "cleanup-site"
        );
    }

    #[tokio::test]
    async fn create_node_hashes_bootstrap_token_before_persisting() {
        let service = HubService::new_memory();
        let created = service
            .create_node(CreateNodeRequest {
                node_code: "hash-node".to_string(),
                name: "Hash Node".to_string(),
                region: "cn-east".to_string(),
                idc: "lab".to_string(),
                labels: json!({}),
            })
            .await
            .unwrap();

        let stored = service.repo.node(created.node_id).await.unwrap().unwrap();
        assert_ne!(
            stored.bootstrap_token.as_deref(),
            Some(created.bootstrap_token.as_str())
        );

        service
            .register_node(NodeRegisterRequest {
                node_code: "hash-node".to_string(),
                bootstrap_token: created.bootstrap_token,
                hostname: "hash-node.local".to_string(),
                public_ip: None,
                private_ip: Some("10.0.0.8".to_string()),
                agent_version: "0.1.0".to_string(),
            })
            .await
            .unwrap();

        let stored = service.repo.node(created.node_id).await.unwrap().unwrap();
        assert!(stored.bootstrap_token.is_none());
        assert_eq!(stored.hostname.as_deref(), Some("hash-node.local"));
    }

    #[tokio::test]
    async fn heartbeat_keeps_maintenance_nodes_in_maintenance() {
        let repo = MemoryRepository::default();
        let node = NodeRecord {
            identity: NodeIdentity {
                id: Uuid::new_v4(),
                node_code: "maintenance-node".to_string(),
                name: "Maintenance Node".to_string(),
                region: "cn-east".to_string(),
                idc: "lab".to_string(),
                status: NodeStatus::Maintenance,
            },
            runtime: NodeRuntimeState {
                active_config_version: None,
                last_seen_at: None,
                site_count: 0,
                health_score: 100,
            },
            labels: json!({}),
            hostname: Some("maintenance-node.local".to_string()),
            public_ip: None,
            private_ip: None,
            bootstrap_token: None,
            pingora_version: None,
            agent_version: None,
        };
        repo.save_node(&node).await.unwrap();

        repo.record_heartbeat(&HeartbeatRecord {
            node_id: node.identity.id,
            pingora_version: "0.5.0".to_string(),
            agent_version: "0.1.0".to_string(),
            active_config_version: Some("rel-maint-001".to_string()),
            site_count: 0,
            cpu_usage: 0.0,
            mem_usage: 0.0,
            disk_usage: 0.0,
            health_score: 88,
            reported_at: Utc::now(),
        })
        .await
        .unwrap();

        let stored = repo.node(node.identity.id).await.unwrap().unwrap();
        assert_eq!(stored.identity.status, NodeStatus::Maintenance);
        assert_eq!(
            stored.runtime.active_config_version.as_deref(),
            Some("rel-maint-001")
        );
        assert_eq!(stored.runtime.health_score, 88);
    }

    #[tokio::test]
    async fn node_operation_flow_dispatches_and_records_result() {
        let repo = Arc::new(MemoryRepository::default());
        let service = HubService {
            repo: repo.clone(),
            tokens: Arc::new(MemoryTokenStore::default()),
            release_counter: Arc::new(AtomicU64::new(1)),
            cert_auto_renew_before_days: CERT_AUTO_RENEW_BEFORE_DAYS,
        };

        let node = NodeRecord {
            identity: NodeIdentity {
                id: Uuid::new_v4(),
                node_code: "ops-node".to_string(),
                name: "Ops Node".to_string(),
                region: "cn-east".to_string(),
                idc: "lab".to_string(),
                status: NodeStatus::Online,
            },
            runtime: NodeRuntimeState {
                active_config_version: None,
                last_seen_at: Some(Utc::now()),
                site_count: 0,
                health_score: 100,
            },
            labels: json!({}),
            hostname: None,
            public_ip: None,
            private_ip: None,
            bootstrap_token: None,
            pingora_version: Some("0.5.0".to_string()),
            agent_version: Some("0.1.0".to_string()),
        };
        repo.save_node(&node).await.unwrap();

        let template = service
            .create_operation_template(CreateOperationTemplateRequest {
                name: "vip-bind".to_string(),
                operation_type: "vip_bind".to_string(),
                command_template: "ip addr add {{vip}} dev {{iface}}".to_string(),
                allowed_params: vec!["vip".to_string(), "iface".to_string()],
                timeout_seconds: 30,
                run_as_user: "root".to_string(),
                approval_required: false,
            })
            .await
            .unwrap();

        let operation = service
            .create_node_operation(
                node.identity.id,
                CreateNodeOperationRequest {
                    template_id: template.template_id,
                    input_params: json!({
                        "vip": "10.10.10.10/32",
                        "iface": "eth1"
                    }),
                    approval_ticket: None,
                },
                "admin:test",
            )
            .await
            .unwrap();
        assert_eq!(operation.exec_status, "approved");

        let claimed = service
            .claim_next_node_operation(node.identity.id)
            .await
            .unwrap()
            .unwrap();
        assert!(claimed.rendered_command.contains("10.10.10.10/32"));
        assert!(claimed.rendered_command.contains("eth1"));

        let result = service
            .report_node_operation_result(
                claimed.operation_id,
                OperationResultRequest {
                    node_id: node.identity.id,
                    exec_status: "success".to_string(),
                    exit_code: 0,
                    stdout: "vip bound".to_string(),
                    stderr: String::new(),
                    finished_at: Utc::now(),
                },
            )
            .await
            .unwrap();
        assert_eq!(result.exec_status, "success");

        let listed = service
            .list_node_operations(node.identity.id)
            .await
            .unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].exec_status, "success");
        assert_eq!(listed[0].stdout_log.as_deref(), Some("vip bound"));
    }
}
