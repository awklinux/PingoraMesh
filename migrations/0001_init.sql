CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE TYPE node_status AS ENUM (
    'pending',
    'online',
    'suspect',
    'offline',
    'maintenance'
);

CREATE TYPE site_status AS ENUM (
    'draft',
    'published',
    'disabled'
);

CREATE TYPE release_status AS ENUM (
    'draft',
    'pending',
    'publishing',
    'success',
    'failed',
    'rolled_back'
);

CREATE TYPE apply_status AS ENUM (
    'pending',
    'downloading',
    'applying',
    'success',
    'failed',
    'rolled_back'
);

CREATE TYPE certificate_status AS ENUM (
    'active',
    'expired',
    'revoked',
    'staging'
);

CREATE TYPE dns_provider_type AS ENUM (
    'cloudflare',
    'alidns',
    'route53',
    'dnspod',
    'custom'
);

CREATE TYPE dns_record_type AS ENUM (
    'A',
    'AAAA',
    'CNAME',
    'TXT',
    'MX',
    'NS',
    'SRV'
);

CREATE TYPE dns_change_status AS ENUM (
    'pending',
    'applied',
    'failed',
    'rolled_back'
);

CREATE TYPE failover_trigger_mode AS ENUM (
    'manual',
    'semi_auto',
    'auto'
);

CREATE TYPE failover_event_status AS ENUM (
    'detected',
    'prechecking',
    'switching',
    'switched',
    'recovered',
    'failed'
);

CREATE TYPE operation_type AS ENUM (
    'vip_bind',
    'vip_unbind',
    'route_switch',
    'service_reload',
    'custom_template'
);

CREATE TYPE operation_exec_status AS ENUM (
    'pending',
    'approved',
    'dispatching',
    'running',
    'success',
    'failed',
    'cancelled',
    'timeout'
);

CREATE TABLE nodes (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    node_code TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    region TEXT NOT NULL,
    idc TEXT NOT NULL,
    labels JSONB NOT NULL DEFAULT '{}'::jsonb,
    public_ip INET,
    private_ip INET,
    status node_status NOT NULL DEFAULT 'pending',
    pingora_version TEXT,
    agent_version TEXT,
    last_seen_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE node_credentials (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    node_id UUID NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    bootstrap_token_hash TEXT,
    client_id TEXT NOT NULL UNIQUE,
    client_secret_hash TEXT,
    mtls_cert_fingerprint TEXT,
    expires_at TIMESTAMPTZ,
    rotated_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (node_id)
);

CREATE TABLE node_heartbeats (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    node_id UUID NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    cpu_usage NUMERIC(5, 2),
    mem_usage NUMERIC(5, 2),
    disk_usage NUMERIC(5, 2),
    load_status JSONB NOT NULL DEFAULT '{}'::jsonb,
    active_config_version TEXT,
    site_count INTEGER NOT NULL DEFAULT 0,
    health_score SMALLINT NOT NULL DEFAULT 100,
    reported_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE sites (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    site_code TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    domain TEXT NOT NULL,
    listen_port INTEGER NOT NULL CHECK (listen_port > 0 AND listen_port < 65536),
    protocol TEXT NOT NULL,
    tls_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    status site_status NOT NULL DEFAULT 'draft',
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE site_configs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    site_id UUID NOT NULL REFERENCES sites(id) ON DELETE CASCADE,
    version INTEGER NOT NULL,
    config_json JSONB NOT NULL,
    config_hash TEXT NOT NULL,
    created_by TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (site_id, version)
);

CREATE TABLE site_node_bindings (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    site_id UUID NOT NULL REFERENCES sites(id) ON DELETE CASCADE,
    node_id UUID NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'active',
    bind_mode TEXT NOT NULL DEFAULT 'manual',
    binding_role TEXT NOT NULL DEFAULT 'primary',
    priority INTEGER NOT NULL DEFAULT 100,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (site_id, node_id)
);

CREATE TABLE certificates (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    cert_code TEXT NOT NULL UNIQUE,
    common_name TEXT NOT NULL,
    sans JSONB NOT NULL DEFAULT '[]'::jsonb,
    issuer TEXT,
    not_before TIMESTAMPTZ,
    not_after TIMESTAMPTZ,
    fingerprint_sha256 TEXT NOT NULL UNIQUE,
    status certificate_status NOT NULL DEFAULT 'staging',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE certificate_versions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    certificate_id UUID NOT NULL REFERENCES certificates(id) ON DELETE CASCADE,
    version INTEGER NOT NULL,
    cert_pem TEXT NOT NULL,
    key_pem_encrypted TEXT NOT NULL,
    chain_pem TEXT,
    kms_key_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (certificate_id, version)
);

CREATE TABLE site_cert_bindings (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    site_id UUID NOT NULL REFERENCES sites(id) ON DELETE CASCADE,
    certificate_id UUID NOT NULL REFERENCES certificates(id) ON DELETE CASCADE,
    version INTEGER NOT NULL,
    is_default BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE config_releases (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    release_code TEXT NOT NULL UNIQUE,
    scope_type TEXT NOT NULL,
    scope_id UUID,
    release_type TEXT NOT NULL DEFAULT 'publish',
    release_version TEXT NOT NULL,
    manifest_json JSONB NOT NULL,
    manifest_hash TEXT NOT NULL,
    reason TEXT,
    status release_status NOT NULL DEFAULT 'draft',
    published_at TIMESTAMPTZ,
    created_by TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE node_release_status (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    release_id UUID NOT NULL REFERENCES config_releases(id) ON DELETE CASCADE,
    node_id UUID NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    target_version TEXT NOT NULL,
    current_version TEXT,
    apply_status apply_status NOT NULL DEFAULT 'pending',
    apply_message TEXT,
    acked_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (release_id, node_id)
);

CREATE TABLE audit_logs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    operator_id TEXT NOT NULL,
    action TEXT NOT NULL,
    resource_type TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    before_data JSONB,
    after_data JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE dns_providers (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE,
    provider_type dns_provider_type NOT NULL,
    api_endpoint TEXT,
    credential_encrypted TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE dns_zones (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    provider_id UUID NOT NULL REFERENCES dns_providers(id) ON DELETE CASCADE,
    zone_name TEXT NOT NULL,
    external_zone_id TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (provider_id, zone_name)
);

CREATE TABLE dns_records (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    zone_id UUID NOT NULL REFERENCES dns_zones(id) ON DELETE CASCADE,
    record_type dns_record_type NOT NULL,
    host TEXT NOT NULL,
    value TEXT NOT NULL,
    ttl INTEGER NOT NULL DEFAULT 60,
    routing_policy JSONB NOT NULL DEFAULT '{}'::jsonb,
    status TEXT NOT NULL DEFAULT 'active',
    external_record_id TEXT,
    last_synced_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE dns_change_logs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    zone_id UUID NOT NULL REFERENCES dns_zones(id) ON DELETE CASCADE,
    record_id UUID REFERENCES dns_records(id) ON DELETE SET NULL,
    change_type TEXT NOT NULL,
    before_data JSONB,
    after_data JSONB,
    change_status dns_change_status NOT NULL DEFAULT 'pending',
    operator_id TEXT NOT NULL,
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE certificate_orders (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    site_id UUID NOT NULL REFERENCES sites(id) ON DELETE CASCADE,
    certificate_id UUID REFERENCES certificates(id) ON DELETE SET NULL,
    zone_id UUID REFERENCES dns_zones(id) ON DELETE SET NULL,
    order_type TEXT NOT NULL DEFAULT 'issue',
    acme_provider TEXT NOT NULL DEFAULT 'letsencrypt',
    challenge_type TEXT NOT NULL DEFAULT 'dns-01',
    challenge_payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    order_status TEXT NOT NULL DEFAULT 'pending',
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE failover_policies (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    scope_type TEXT NOT NULL,
    scope_id UUID NOT NULL,
    primary_node_id UUID NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    standby_node_id UUID NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    trigger_mode failover_trigger_mode NOT NULL DEFAULT 'manual',
    failure_threshold INTEGER NOT NULL DEFAULT 3,
    recover_threshold INTEGER NOT NULL DEFAULT 5,
    precheck_policy JSONB NOT NULL DEFAULT '{}'::jsonb,
    status TEXT NOT NULL DEFAULT 'active',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (primary_node_id <> standby_node_id)
);

CREATE TABLE failover_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    policy_id UUID NOT NULL REFERENCES failover_policies(id) ON DELETE CASCADE,
    site_id UUID REFERENCES sites(id) ON DELETE SET NULL,
    source_node_id UUID NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    target_node_id UUID NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    trigger_reason TEXT NOT NULL,
    event_status failover_event_status NOT NULL DEFAULT 'detected',
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ,
    CHECK (source_node_id <> target_node_id)
);

CREATE TABLE operation_templates (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE,
    operation_type operation_type NOT NULL,
    command_template TEXT NOT NULL,
    allowed_params JSONB NOT NULL DEFAULT '[]'::jsonb,
    timeout_seconds INTEGER NOT NULL DEFAULT 60,
    run_as_user TEXT NOT NULL DEFAULT 'root',
    approval_required BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE node_operations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    node_id UUID NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    template_id UUID NOT NULL REFERENCES operation_templates(id) ON DELETE RESTRICT,
    event_id UUID REFERENCES failover_events(id) ON DELETE SET NULL,
    input_params JSONB NOT NULL DEFAULT '{}'::jsonb,
    exec_status operation_exec_status NOT NULL DEFAULT 'pending',
    requested_by TEXT NOT NULL DEFAULT 'system',
    approved_by TEXT,
    exit_code INTEGER,
    stdout_log TEXT,
    stderr_log TEXT,
    started_at TIMESTAMPTZ,
    finished_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_nodes_status ON nodes(status);
CREATE INDEX idx_nodes_last_seen_at ON nodes(last_seen_at DESC);
CREATE INDEX idx_node_heartbeats_node_id_reported_at ON node_heartbeats(node_id, reported_at DESC);
CREATE INDEX idx_sites_status ON sites(status);
CREATE INDEX idx_site_configs_site_id_version ON site_configs(site_id, version DESC);
CREATE INDEX idx_site_node_bindings_node_id ON site_node_bindings(node_id);
CREATE INDEX idx_certificates_not_after ON certificates(not_after);
CREATE INDEX idx_config_releases_scope ON config_releases(scope_type, scope_id);
CREATE INDEX idx_config_releases_status ON config_releases(status);
CREATE INDEX idx_node_release_status_node_id ON node_release_status(node_id);
CREATE INDEX idx_dns_zones_provider_id ON dns_zones(provider_id);
CREATE INDEX idx_dns_records_zone_id_host ON dns_records(zone_id, host);
CREATE INDEX idx_dns_change_logs_zone_id_created_at ON dns_change_logs(zone_id, created_at DESC);
CREATE INDEX idx_certificate_orders_site_id ON certificate_orders(site_id);
CREATE INDEX idx_failover_policies_scope ON failover_policies(scope_type, scope_id);
CREATE INDEX idx_failover_events_policy_id_started_at ON failover_events(policy_id, started_at DESC);
CREATE INDEX idx_node_operations_node_id_created_at ON node_operations(node_id, created_at DESC);
