use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use pingorahub_protocol::{
    ApiResponse, ConfigPackageResponse, LatestReleaseResponse, NodeHeartbeatRequest,
    NodeHeartbeatResponse, NodeRefreshRequest, NodeRefreshResponse, OperationResultRequest,
    OperationResultResponse, PendingOperationResponse, ReleaseAckRequest, ReleaseAckResponse,
};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::process::Command;
use tracing::{info, warn};
use uuid::Uuid;

#[derive(Debug, Clone)]
struct AgentConfig {
    hub_base_url: String,
    registration_path: PathBuf,
    state_path: PathBuf,
    data_dir: PathBuf,
    pingora_version: String,
    agent_version: String,
    heartbeat_interval_seconds: u64,
    apply_command: Option<String>,
    allow_noop_apply: bool,
    apply_timeout_seconds: u64,
    healthcheck_url: Option<String>,
    healthcheck_expected_status: Option<u16>,
    healthcheck_timeout_seconds: u64,
    healthcheck_retries: u32,
    healthcheck_interval_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RegistrationMetadata {
    node_id: Uuid,
    node_code: String,
    hostname: String,
    private_ip: String,
    refresh_token: String,
    expires_at: String,
    registration_mode: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct LocalAgentState {
    current_release_id: Option<Uuid>,
    current_release_version: Option<String>,
    current_config_hash: Option<String>,
    site_count: u32,
    last_acknowledged_release_id: Option<Uuid>,
    last_heartbeat_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
struct Session {
    access_token: String,
    expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
struct ReleaseArtifacts {
    release_dir: PathBuf,
    package_path: PathBuf,
    manifest_path: PathBuf,
    rendered_config_path: PathBuf,
    certificates_path: PathBuf,
}

#[derive(Debug)]
enum SyncAction {
    NoRelease,
    Idle,
    Applied(String),
    Acked(String),
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let config = AgentConfig::from_env();
    config.ensure_dirs()?;
    let client = Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .context("failed to build node-agent http client")?;

    let mut registration = RegistrationMetadata::load(&config.registration_path)?;
    let mut state = LocalAgentState::load(&config.state_path)?;
    let mut session = refresh_session(&client, &config, &mut registration).await?;

    info!(
        node_id = %registration.node_id,
        node_code = %registration.node_code,
        hub = %config.hub_base_url,
        "node-agent started"
    );

    loop {
        if session.should_refresh() {
            session = refresh_session(&client, &config, &mut registration).await?;
        }

        let heartbeat_response = match heartbeat(&client, &config, &registration, &state, &session)
            .await
        {
            Ok(response) => response,
            Err(error) if is_auth_error(&error) => {
                warn!(node_id = %registration.node_id, error = %error, "heartbeat rejected, refreshing token");
                session = refresh_session(&client, &config, &mut registration).await?;
                heartbeat(&client, &config, &registration, &state, &session).await?
            }
            Err(error) => return Err(error),
        };

        state.last_heartbeat_at = Some(Utc::now());
        state.persist(&config.state_path)?;

        match sync_latest_release(&client, &config, &registration, &mut state, &mut session).await {
            Ok(SyncAction::Applied(version)) => {
                info!(version, node_id = %registration.node_id, "node-agent applied latest release");
            }
            Ok(SyncAction::Acked(version)) => {
                info!(version, node_id = %registration.node_id, "node-agent acknowledged existing release");
            }
            Ok(SyncAction::Idle | SyncAction::NoRelease) => {}
            Err(error) if is_auth_error(&error) => {
                warn!(node_id = %registration.node_id, error = %error, "release sync rejected, refreshing token");
                session = refresh_session(&client, &config, &mut registration).await?;
                let _ =
                    sync_latest_release(&client, &config, &registration, &mut state, &mut session)
                        .await?;
            }
            Err(error) => {
                warn!(node_id = %registration.node_id, error = %error, "release sync failed");
            }
        }

        match sync_pending_operations(&client, &config, &registration, &session).await {
            Ok(executed) if executed > 0 => {
                info!(executed, node_id = %registration.node_id, "node-agent executed pending operations");
            }
            Ok(_) => {}
            Err(error) if is_auth_error(&error) => {
                warn!(node_id = %registration.node_id, error = %error, "operation sync rejected, refreshing token");
                session = refresh_session(&client, &config, &mut registration).await?;
                let _ = sync_pending_operations(&client, &config, &registration, &session).await?;
            }
            Err(error) => {
                warn!(node_id = %registration.node_id, error = %error, "operation sync failed");
            }
        }

        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            _ = tokio::time::sleep(Duration::from_secs(
                heartbeat_response
                    .next_heartbeat_after_seconds
                    .max(config.heartbeat_interval_seconds)
            )) => {}
        }
    }

    info!(node_id = %registration.node_id, "node-agent stopped");
    Ok(())
}

impl AgentConfig {
    fn from_env() -> Self {
        let data_dir = std::env::var("PINGORAHUB_NODE_AGENT_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/var/lib/pingorahub/node-agent"));
        let state_path = std::env::var("PINGORAHUB_NODE_AGENT_STATE_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| data_dir.join("state.json"));

        Self {
            hub_base_url: std::env::var("PINGORAHUB_HUB_BASE_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:3000".to_string())
                .trim_end_matches('/')
                .to_string(),
            registration_path: std::env::var("PINGORAHUB_NODE_REGISTRATION_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("/etc/pingorahub/manual-node-registration.json")),
            state_path,
            data_dir,
            pingora_version: std::env::var("PINGORAHUB_NODE_AGENT_PINGORA_VERSION")
                .unwrap_or_else(|_| "manual-pending".to_string()),
            agent_version: std::env::var("PINGORAHUB_NODE_AGENT_VERSION")
                .unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_string()),
            heartbeat_interval_seconds: std::env::var(
                "PINGORAHUB_NODE_AGENT_MIN_HEARTBEAT_SECONDS",
            )
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(15),
            apply_command: optional_env("PINGORAHUB_NODE_APPLY_COMMAND"),
            allow_noop_apply: parse_env_bool("PINGORAHUB_NODE_ALLOW_NOOP_APPLY"),
            apply_timeout_seconds: parse_env_u64("PINGORAHUB_NODE_APPLY_TIMEOUT_SECONDS", 30),
            healthcheck_url: optional_env("PINGORAHUB_NODE_APPLY_HEALTHCHECK_URL"),
            healthcheck_expected_status: std::env::var(
                "PINGORAHUB_NODE_APPLY_HEALTHCHECK_EXPECT_STATUS",
            )
            .ok()
            .and_then(|value| value.parse::<u16>().ok()),
            healthcheck_timeout_seconds: parse_env_u64(
                "PINGORAHUB_NODE_APPLY_HEALTHCHECK_TIMEOUT_SECONDS",
                10,
            ),
            healthcheck_retries: parse_env_u32("PINGORAHUB_NODE_APPLY_HEALTHCHECK_RETRIES", 3),
            healthcheck_interval_seconds: parse_env_u64(
                "PINGORAHUB_NODE_APPLY_HEALTHCHECK_INTERVAL_SECONDS",
                2,
            ),
        }
    }

    fn ensure_dirs(&self) -> Result<()> {
        fs::create_dir_all(&self.data_dir)
            .with_context(|| format!("failed to create {}", self.data_dir.display()))?;
        if let Some(parent) = self.state_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        fs::create_dir_all(self.releases_dir())
            .with_context(|| format!("failed to create {}", self.releases_dir().display()))?;
        Ok(())
    }

    fn releases_dir(&self) -> PathBuf {
        self.data_dir.join("releases")
    }

    fn current_release_version_path(&self) -> PathBuf {
        self.data_dir.join("current-release-version")
    }

    fn current_manifest_path(&self) -> PathBuf {
        self.data_dir.join("current-manifest.json")
    }

    fn current_rendered_config_path(&self) -> PathBuf {
        self.data_dir.join("current-rendered-config.json")
    }

    fn current_certificates_path(&self) -> PathBuf {
        self.data_dir.join("current-certificates.json")
    }
}

impl RegistrationMetadata {
    fn load(path: &Path) -> Result<Self> {
        let raw = fs::read_to_string(path).with_context(|| {
            format!(
                "failed to read registration metadata from {}",
                path.display()
            )
        })?;
        serde_json::from_str(&raw).context("failed to parse registration metadata json")
    }

    fn persist(&self, path: &Path) -> Result<()> {
        let body =
            serde_json::to_string(self).context("failed to serialize registration metadata")?;
        fs::write(path, body).with_context(|| {
            format!(
                "failed to write registration metadata to {}",
                path.display()
            )
        })
    }
}

impl LocalAgentState {
    fn load(path: &Path) -> Result<Self> {
        match fs::read_to_string(path) {
            Ok(raw) => serde_json::from_str(&raw).context("failed to parse node-agent state json"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error).with_context(|| {
                format!("failed to read node-agent state from {}", path.display())
            }),
        }
    }

    fn persist(&self, path: &Path) -> Result<()> {
        let body =
            serde_json::to_string_pretty(self).context("failed to serialize node-agent state")?;
        fs::write(path, body)
            .with_context(|| format!("failed to write node-agent state to {}", path.display()))
    }
}

impl Session {
    fn should_refresh(&self) -> bool {
        self.expires_at <= Utc::now() + chrono::Duration::minutes(5)
    }
}

async fn refresh_session(
    client: &Client,
    config: &AgentConfig,
    registration: &mut RegistrationMetadata,
) -> Result<Session> {
    let response: NodeRefreshResponse = post_json(
        client,
        &format!("{}/api/node/auth/refresh", config.hub_base_url),
        &NodeRefreshRequest {
            node_id: registration.node_id,
            refresh_token: registration.refresh_token.clone(),
        },
        None,
    )
    .await?;

    registration.refresh_token = response.refresh_token;
    registration.expires_at = response.expires_at.to_rfc3339();
    registration.persist(&config.registration_path)?;

    Ok(Session {
        access_token: response.access_token,
        expires_at: response.expires_at,
    })
}

async fn heartbeat(
    client: &Client,
    config: &AgentConfig,
    registration: &RegistrationMetadata,
    state: &LocalAgentState,
    session: &Session,
) -> Result<NodeHeartbeatResponse> {
    post_json(
        client,
        &format!("{}/api/node/heartbeat", config.hub_base_url),
        &NodeHeartbeatRequest {
            node_id: registration.node_id,
            pingora_version: config.pingora_version.clone(),
            agent_version: config.agent_version.clone(),
            active_config_version: state.current_release_version.clone(),
            site_count: state.site_count,
            cpu_usage: 0.0,
            mem_usage: 0.0,
            disk_usage: 0.0,
            health_score: 100,
        },
        Some(&session.access_token),
    )
    .await
}

async fn sync_latest_release(
    client: &Client,
    config: &AgentConfig,
    registration: &RegistrationMetadata,
    state: &mut LocalAgentState,
    session: &mut Session,
) -> Result<SyncAction> {
    let latest = match get_json::<LatestReleaseResponse>(
        client,
        &format!(
            "{}/api/node/config/releases/latest?node_id={}",
            config.hub_base_url, registration.node_id
        ),
        Some(&session.access_token),
    )
    .await
    {
        Ok(response) => response,
        Err(error) if is_not_found(&error) => return Ok(SyncAction::NoRelease),
        Err(error) => return Err(error),
    };

    if state.current_release_version.as_deref() != Some(latest.release_version.as_str()) {
        let previous_version = reported_current_version(state, &latest.release_version);
        report_release_status(
            client,
            config,
            registration,
            session,
            latest.release_id,
            "downloading",
            &previous_version,
            Some(format!("node-agent downloading {}", latest.release_version)),
        )
        .await?;

        let package: ConfigPackageResponse = get_json(
            client,
            &format!("{}{}", config.hub_base_url, latest.download_url),
            Some(&session.access_token),
        )
        .await?;

        let artifacts = persist_release_artifacts(config, &package)?;
        report_release_status(
            client,
            config,
            registration,
            session,
            latest.release_id,
            "applying",
            &previous_version,
            Some(format!(
                "release artifacts staged at {}; starting apply pipeline",
                artifacts.release_dir.display()
            )),
        )
        .await?;

        let apply_message =
            match apply_release_package(client, config, registration, &package, &artifacts, state)
                .await
            {
                Ok(message) => message,
                Err(error) => {
                    let failure_message = truncate_message(&format!(
                        "release {} apply failed: {error:#}",
                        package.release_version
                    ));
                    let _ = report_release_status(
                        client,
                        config,
                        registration,
                        session,
                        latest.release_id,
                        "failed",
                        &previous_version,
                        Some(failure_message),
                    )
                    .await;
                    return Err(error);
                }
            };

        activate_release_artifacts(config, &package, &artifacts)?;
        state.current_release_id = Some(package.release_id);
        state.current_release_version = Some(package.release_version.clone());
        state.current_config_hash = Some(latest.config_hash);
        state.site_count = package
            .manifest
            .get("sites")
            .and_then(Value::as_array)
            .map(|sites| sites.len() as u32)
            .unwrap_or(0);

        acknowledge_release(
            client,
            config,
            registration,
            session,
            &package,
            Some(apply_message),
        )
        .await?;
        state.last_acknowledged_release_id = Some(package.release_id);
        state.persist(&config.state_path)?;

        return Ok(SyncAction::Applied(package.release_version));
    }

    if state.last_acknowledged_release_id != Some(latest.release_id) {
        let current_version = state
            .current_release_version
            .clone()
            .ok_or_else(|| anyhow!("missing current release version for ack"))?;
        post_json::<ReleaseAckRequest, ReleaseAckResponse>(
            client,
            &format!(
                "{}/api/node/releases/{}/ack",
                config.hub_base_url, latest.release_id
            ),
            &ReleaseAckRequest {
                node_id: registration.node_id,
                apply_status: "success".to_string(),
                current_version: current_version.clone(),
                message: Some("node-agent confirmed active release state".to_string()),
            },
            Some(&session.access_token),
        )
        .await?;
        state.last_acknowledged_release_id = Some(latest.release_id);
        state.persist(&config.state_path)?;
        return Ok(SyncAction::Acked(current_version));
    }

    Ok(SyncAction::Idle)
}

async fn sync_pending_operations(
    client: &Client,
    config: &AgentConfig,
    registration: &RegistrationMetadata,
    session: &Session,
) -> Result<u32> {
    let mut executed = 0_u32;

    loop {
        let operation = match get_json::<PendingOperationResponse>(
            client,
            &format!(
                "{}/api/node/operations/pending?node_id={}",
                config.hub_base_url, registration.node_id
            ),
            Some(&session.access_token),
        )
        .await
        {
            Ok(operation) => operation,
            Err(error) if is_not_found(&error) => break,
            Err(error) => return Err(error),
        };

        let result = run_node_operation(registration, &operation).await;
        let (exec_status, exit_code, stdout, stderr) = match result {
            Ok(stdout) => ("success".to_string(), 0, stdout, String::new()),
            Err(error) => (
                classify_operation_error(&error),
                1,
                String::new(),
                truncate_message(&format!("{error:#}")),
            ),
        };

        post_json::<OperationResultRequest, OperationResultResponse>(
            client,
            &format!(
                "{}/api/node/operations/{}/result",
                config.hub_base_url, operation.operation_id
            ),
            &OperationResultRequest {
                node_id: registration.node_id,
                exec_status,
                exit_code,
                stdout,
                stderr,
                finished_at: Utc::now(),
            },
            Some(&session.access_token),
        )
        .await?;

        executed += 1;
    }

    Ok(executed)
}

async fn acknowledge_release(
    client: &Client,
    config: &AgentConfig,
    registration: &RegistrationMetadata,
    session: &Session,
    package: &ConfigPackageResponse,
    message: Option<String>,
) -> Result<()> {
    report_release_status(
        client,
        config,
        registration,
        session,
        package.release_id,
        "success",
        &package.release_version,
        message,
    )
    .await?;
    Ok(())
}

async fn run_node_operation(
    registration: &RegistrationMetadata,
    operation: &PendingOperationResponse,
) -> Result<String> {
    let current_user = current_process_user();
    if !operation.run_as_user.trim().is_empty() && operation.run_as_user != current_user {
        return Err(anyhow!(
            "operation {} requires run_as_user={}, current user={}",
            operation.template_name,
            operation.run_as_user,
            current_user
        ));
    }

    let output = tokio::time::timeout(
        Duration::from_secs(operation.timeout_seconds.max(1)),
        Command::new("sh")
            .arg("-lc")
            .arg(operation.rendered_command.trim())
            .kill_on_drop(true)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("PINGORAHUB_NODE_ID", registration.node_id.to_string())
            .env("PINGORAHUB_NODE_CODE", &registration.node_code)
            .env("PINGORAHUB_NODE_HOSTNAME", &registration.hostname)
            .env("PINGORAHUB_NODE_PRIVATE_IP", &registration.private_ip)
            .env(
                "PINGORAHUB_OPERATION_ID",
                operation.operation_id.to_string(),
            )
            .env(
                "PINGORAHUB_OPERATION_TEMPLATE_ID",
                operation.template_id.to_string(),
            )
            .env(
                "PINGORAHUB_OPERATION_TEMPLATE_NAME",
                &operation.template_name,
            )
            .env("PINGORAHUB_OPERATION_TYPE", &operation.operation_type)
            .output(),
    )
    .await
    .with_context(|| {
        format!(
            "operation {} timed out after {}s",
            operation.template_name, operation.timeout_seconds
        )
    })?
    .context("failed to execute node operation command")?;

    let stdout = truncate_message(&String::from_utf8_lossy(&output.stdout));
    let stderr = truncate_message(&String::from_utf8_lossy(&output.stderr));

    if !output.status.success() {
        return Err(anyhow!(
            "node operation exited with {}; stdout={}; stderr={}",
            output
                .status
                .code()
                .map(|code| code.to_string())
                .unwrap_or_else(|| "signal".to_string()),
            if stdout.is_empty() {
                "-"
            } else {
                stdout.as_str()
            },
            if stderr.is_empty() {
                "-"
            } else {
                stderr.as_str()
            },
        ));
    }

    if stdout.is_empty() {
        Ok(format!("operation {} completed", operation.template_name))
    } else {
        Ok(stdout)
    }
}

fn persist_release_artifacts(
    config: &AgentConfig,
    package: &ConfigPackageResponse,
) -> Result<ReleaseArtifacts> {
    let release_dir = config.releases_dir().join(&package.release_version);
    fs::create_dir_all(&release_dir)
        .with_context(|| format!("failed to create {}", release_dir.display()))?;

    let package_path = release_dir.join("package.json");
    let manifest_path = release_dir.join("manifest.json");
    let rendered_config_path = release_dir.join("rendered-config.json");
    let certificates_path = release_dir.join("certificates.json");

    fs::write(
        &package_path,
        serde_json::to_vec_pretty(package).context("failed to serialize package json")?,
    )
    .with_context(|| {
        format!(
            "failed to write package.json for {}",
            package.release_version
        )
    })?;
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&package.manifest)
            .context("failed to serialize manifest json")?,
    )
    .with_context(|| {
        format!(
            "failed to write manifest.json for {}",
            package.release_version
        )
    })?;
    fs::write(
        &rendered_config_path,
        serde_json::to_vec_pretty(&package.rendered_config)
            .context("failed to serialize rendered config json")?,
    )
    .with_context(|| {
        format!(
            "failed to write rendered-config.json for {}",
            package.release_version
        )
    })?;
    fs::write(
        &certificates_path,
        serde_json::to_vec_pretty(&package.certificates)
            .context("failed to serialize certificates json")?,
    )
    .with_context(|| {
        format!(
            "failed to write certificates.json for {}",
            package.release_version
        )
    })?;

    Ok(ReleaseArtifacts {
        release_dir,
        package_path,
        manifest_path,
        rendered_config_path,
        certificates_path,
    })
}

fn activate_release_artifacts(
    config: &AgentConfig,
    package: &ConfigPackageResponse,
    artifacts: &ReleaseArtifacts,
) -> Result<()> {
    fs::write(
        config.current_release_version_path(),
        &package.release_version,
    )
    .with_context(|| {
        format!(
            "failed to write {}",
            config.current_release_version_path().display()
        )
    })?;
    fs::write(
        config.current_manifest_path(),
        fs::read(&artifacts.manifest_path).with_context(|| {
            format!(
                "failed to read staged manifest {}",
                artifacts.manifest_path.display()
            )
        })?,
    )
    .with_context(|| {
        format!(
            "failed to write {}",
            config.current_manifest_path().display()
        )
    })?;
    fs::write(
        config.current_rendered_config_path(),
        fs::read(&artifacts.rendered_config_path).with_context(|| {
            format!(
                "failed to read staged rendered config {}",
                artifacts.rendered_config_path.display()
            )
        })?,
    )
    .with_context(|| {
        format!(
            "failed to write {}",
            config.current_rendered_config_path().display()
        )
    })?;
    fs::write(
        config.current_certificates_path(),
        fs::read(&artifacts.certificates_path).with_context(|| {
            format!(
                "failed to read staged certificates {}",
                artifacts.certificates_path.display()
            )
        })?,
    )
    .with_context(|| {
        format!(
            "failed to write {}",
            config.current_certificates_path().display()
        )
    })?;

    Ok(())
}

async fn apply_release_package(
    client: &Client,
    config: &AgentConfig,
    registration: &RegistrationMetadata,
    package: &ConfigPackageResponse,
    artifacts: &ReleaseArtifacts,
    state: &LocalAgentState,
) -> Result<String> {
    ensure_apply_mode(config)?;

    let mut notes = vec![format!(
        "artifacts staged at {}",
        artifacts.release_dir.display()
    )];

    if let Some(command) = config.apply_command.as_deref() {
        let command_message = run_apply_command(
            config,
            registration,
            package,
            artifacts,
            state.current_release_version.as_deref(),
            command,
        )
        .await?;
        notes.push(command_message);
    } else {
        notes.push("no apply command configured; noop apply explicitly allowed".to_string());
    }

    if config.healthcheck_url.is_some() {
        let health_message = run_healthcheck(client, config).await?;
        notes.push(health_message);
    } else {
        notes.push("no health check configured".to_string());
    }

    Ok(notes.join("; "))
}

fn ensure_apply_mode(config: &AgentConfig) -> Result<()> {
    if config.apply_command.is_none() && !config.allow_noop_apply {
        return Err(anyhow!(
            "PINGORAHUB_NODE_APPLY_COMMAND is required unless PINGORAHUB_NODE_ALLOW_NOOP_APPLY=true"
        ));
    }
    Ok(())
}

async fn run_apply_command(
    config: &AgentConfig,
    registration: &RegistrationMetadata,
    package: &ConfigPackageResponse,
    artifacts: &ReleaseArtifacts,
    previous_version: Option<&str>,
    command: &str,
) -> Result<String> {
    let trimmed_command = command.trim();
    if trimmed_command.is_empty() {
        return Ok("apply command is empty; skipped external apply step".to_string());
    }

    let mut process = Command::new("sh");
    process
        .arg("-lc")
        .arg(trimmed_command)
        .kill_on_drop(true)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("PINGORAHUB_NODE_ID", registration.node_id.to_string())
        .env("PINGORAHUB_NODE_CODE", &registration.node_code)
        .env("PINGORAHUB_NODE_HOSTNAME", &registration.hostname)
        .env("PINGORAHUB_NODE_PRIVATE_IP", &registration.private_ip)
        .env("PINGORAHUB_DATA_DIR", &config.data_dir)
        .env("PINGORAHUB_RELEASE_ID", package.release_id.to_string())
        .env("PINGORAHUB_RELEASE_VERSION", &package.release_version)
        .env("PINGORAHUB_RELEASE_DIR", &artifacts.release_dir)
        .env("PINGORAHUB_RELEASE_PACKAGE_PATH", &artifacts.package_path)
        .env("PINGORAHUB_RELEASE_MANIFEST_PATH", &artifacts.manifest_path)
        .env(
            "PINGORAHUB_RELEASE_RENDERED_CONFIG_PATH",
            &artifacts.rendered_config_path,
        )
        .env(
            "PINGORAHUB_RELEASE_CERTIFICATES_PATH",
            &artifacts.certificates_path,
        );

    if let Some(version) = previous_version {
        process.env("PINGORAHUB_PREVIOUS_RELEASE_VERSION", version);
    }

    let output = tokio::time::timeout(
        Duration::from_secs(config.apply_timeout_seconds),
        process.output(),
    )
    .await
    .with_context(|| {
        format!(
            "apply command timed out after {}s",
            config.apply_timeout_seconds
        )
    })?
    .context("failed to execute apply command")?;

    let stdout = truncate_message(&String::from_utf8_lossy(&output.stdout));
    let stderr = truncate_message(&String::from_utf8_lossy(&output.stderr));

    if !output.status.success() {
        return Err(anyhow!(
            "apply command exited with {}; stdout={}; stderr={}",
            output
                .status
                .code()
                .map(|code| code.to_string())
                .unwrap_or_else(|| "signal".to_string()),
            if stdout.is_empty() {
                "-"
            } else {
                stdout.as_str()
            },
            if stderr.is_empty() {
                "-"
            } else {
                stderr.as_str()
            },
        ));
    }

    if stdout.is_empty() {
        Ok("apply command completed".to_string())
    } else {
        Ok(format!("apply command completed: {stdout}"))
    }
}

async fn run_healthcheck(client: &Client, config: &AgentConfig) -> Result<String> {
    let healthcheck_url = config
        .healthcheck_url
        .as_deref()
        .ok_or_else(|| anyhow!("health check url is not configured"))?;
    let attempts = config.healthcheck_retries.max(1);
    let mut last_error = String::new();

    for attempt in 1..=attempts {
        match client
            .get(healthcheck_url)
            .timeout(Duration::from_secs(config.healthcheck_timeout_seconds))
            .send()
            .await
        {
            Ok(response) => {
                if healthcheck_status_matches(response.status(), config.healthcheck_expected_status)
                {
                    return Ok(format!(
                        "health check passed on attempt {attempt} with status {}",
                        response.status().as_u16()
                    ));
                }

                last_error = format!(
                    "unexpected status {} on attempt {attempt}",
                    response.status().as_u16()
                );
            }
            Err(error) => {
                last_error = format!("health check attempt {attempt} failed: {error}");
            }
        }

        if attempt < attempts {
            tokio::time::sleep(Duration::from_secs(config.healthcheck_interval_seconds)).await;
        }
    }

    Err(anyhow!(
        "health check {} failed after {} attempt(s): {}",
        healthcheck_url,
        attempts,
        last_error
    ))
}

fn healthcheck_status_matches(status: StatusCode, expected_status: Option<u16>) -> bool {
    if let Some(expected) = expected_status {
        status.as_u16() == expected
    } else {
        status.is_success() || status.is_redirection()
    }
}

async fn report_release_status(
    client: &Client,
    config: &AgentConfig,
    registration: &RegistrationMetadata,
    session: &Session,
    release_id: Uuid,
    apply_status: &str,
    current_version: &str,
    message: Option<String>,
) -> Result<ReleaseAckResponse> {
    post_json::<ReleaseAckRequest, ReleaseAckResponse>(
        client,
        &format!("{}/api/node/releases/{release_id}/ack", config.hub_base_url),
        &ReleaseAckRequest {
            node_id: registration.node_id,
            apply_status: apply_status.to_string(),
            current_version: current_version.to_string(),
            message,
        },
        Some(&session.access_token),
    )
    .await
}

fn reported_current_version(state: &LocalAgentState, fallback: &str) -> String {
    state
        .current_release_version
        .clone()
        .unwrap_or_else(|| fallback.to_string())
}

fn truncate_message(raw: &str) -> String {
    const MAX_LEN: usize = 400;
    let normalized = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= MAX_LEN {
        normalized
    } else {
        let truncated = normalized.chars().take(MAX_LEN).collect::<String>();
        format!("{truncated}...")
    }
}

fn classify_operation_error(error: &anyhow::Error) -> String {
    if error.to_string().contains("timed out") {
        "timeout".to_string()
    } else {
        "failed".to_string()
    }
}

fn current_process_user() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

fn optional_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn parse_env_u64(name: &str, default_value: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default_value)
}

fn parse_env_u32(name: &str, default_value: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(default_value)
}

fn parse_env_bool(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
}

async fn get_json<T: DeserializeOwned>(
    client: &Client,
    url: &str,
    bearer_token: Option<&str>,
) -> Result<T> {
    let mut request = client.get(url);
    if let Some(token) = bearer_token {
        request = request.bearer_auth(token);
    }
    let response = request
        .send()
        .await
        .with_context(|| format!("failed GET {url}"))?;
    parse_response(response).await
}

async fn post_json<Body: Serialize, ResponseBody: DeserializeOwned>(
    client: &Client,
    url: &str,
    body: &Body,
    bearer_token: Option<&str>,
) -> Result<ResponseBody> {
    let mut request = client.post(url).json(body);
    if let Some(token) = bearer_token {
        request = request.bearer_auth(token);
    }
    let response = request
        .send()
        .await
        .with_context(|| format!("failed POST {url}"))?;
    parse_response(response).await
}

async fn parse_response<T: DeserializeOwned>(response: reqwest::Response) -> Result<T> {
    let status = response.status();
    let body = response
        .text()
        .await
        .context("failed to read response body")?;
    if !status.is_success() {
        return Err(anyhow!("http {}: {}", status.as_u16(), body));
    }

    let envelope: ApiResponse<T> =
        serde_json::from_str(&body).context("failed to decode api response envelope")?;
    Ok(envelope.data)
}

fn is_not_found(error: &anyhow::Error) -> bool {
    error
        .to_string()
        .contains(&format!("http {}:", StatusCode::NOT_FOUND.as_u16()))
}

fn is_auth_error(error: &anyhow::Error) -> bool {
    error
        .to_string()
        .contains(&format!("http {}:", StatusCode::UNAUTHORIZED.as_u16()))
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "pingorahub_node_agent=info,info".into()),
        )
        .try_init();
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn loads_default_agent_config() {
        let config = AgentConfig::from_env();
        assert!(!config.hub_base_url.is_empty());
        assert!(config.registration_path.is_absolute());
        assert!(config.apply_command.is_none());
        assert!(!config.allow_noop_apply);
    }

    #[test]
    fn session_refresh_threshold_is_checked() {
        let session = Session {
            access_token: "token".to_string(),
            expires_at: Utc::now() + chrono::Duration::minutes(3),
        };
        assert!(session.should_refresh());
    }

    #[test]
    fn reported_current_version_prefers_active_release() {
        let state = LocalAgentState {
            current_release_version: Some("rel-site-active-001".to_string()),
            ..Default::default()
        };
        assert_eq!(
            reported_current_version(&state, "rel-site-target-002"),
            "rel-site-active-001"
        );
        assert_eq!(
            reported_current_version(&LocalAgentState::default(), "rel-site-target-002"),
            "rel-site-target-002"
        );
    }

    #[test]
    fn truncate_message_compacts_whitespace_and_limits_length() {
        let compact = truncate_message("hello   world\nline");
        assert_eq!(compact, "hello world line");

        let long = "x".repeat(500);
        assert!(truncate_message(&long).len() <= 403);
    }

    #[test]
    fn persist_and_activate_release_artifacts_work() {
        let temp_dir =
            std::env::temp_dir().join(format!("pingorahub-node-agent-{}", Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();

        let config = AgentConfig {
            hub_base_url: "http://127.0.0.1:3000".to_string(),
            registration_path: temp_dir.join("registration.json"),
            state_path: temp_dir.join("state.json"),
            data_dir: temp_dir.clone(),
            pingora_version: "test".to_string(),
            agent_version: "test".to_string(),
            heartbeat_interval_seconds: 15,
            apply_command: None,
            allow_noop_apply: false,
            apply_timeout_seconds: 30,
            healthcheck_url: None,
            healthcheck_expected_status: None,
            healthcheck_timeout_seconds: 10,
            healthcheck_retries: 3,
            healthcheck_interval_seconds: 2,
        };

        let package = ConfigPackageResponse {
            release_id: Uuid::new_v4(),
            release_version: "rel-site-202604140001-001".to_string(),
            manifest: json!({
                "release_id": Uuid::new_v4(),
                "sites": [{ "site_code": "test01" }]
            }),
            rendered_config: json!({
                "site_code": "test01",
                "upstreams": []
            }),
            certificates: json!([]),
            signature: "sig:test".to_string(),
        };

        let artifacts = persist_release_artifacts(&config, &package).unwrap();
        assert!(artifacts.package_path.exists());
        assert!(artifacts.manifest_path.exists());
        assert!(artifacts.rendered_config_path.exists());
        assert!(artifacts.certificates_path.exists());

        activate_release_artifacts(&config, &package, &artifacts).unwrap();
        assert_eq!(
            fs::read_to_string(config.current_release_version_path()).unwrap(),
            package.release_version
        );
        assert!(config.current_manifest_path().exists());
        assert!(config.current_rendered_config_path().exists());
        assert!(config.current_certificates_path().exists());

        fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[test]
    fn apply_release_requires_command_unless_noop_apply_is_allowed() {
        let base = AgentConfig {
            hub_base_url: "http://127.0.0.1:3000".to_string(),
            registration_path: PathBuf::from("/tmp/registration.json"),
            state_path: PathBuf::from("/tmp/state.json"),
            data_dir: PathBuf::from("/tmp/node-agent"),
            pingora_version: "test".to_string(),
            agent_version: "test".to_string(),
            heartbeat_interval_seconds: 15,
            apply_command: None,
            allow_noop_apply: false,
            apply_timeout_seconds: 30,
            healthcheck_url: None,
            healthcheck_expected_status: None,
            healthcheck_timeout_seconds: 10,
            healthcheck_retries: 3,
            healthcheck_interval_seconds: 2,
        };

        let error = ensure_apply_mode(&base).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("PINGORAHUB_NODE_APPLY_COMMAND is required")
        );

        let allowed = AgentConfig {
            allow_noop_apply: true,
            ..base
        };
        assert!(ensure_apply_mode(&allowed).is_ok());
    }
}
