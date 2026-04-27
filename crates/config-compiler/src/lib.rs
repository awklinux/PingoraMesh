use anyhow::{Result, anyhow};
use chrono::Utc;
use pingorahub_domain::{CertificateRef, ReleaseManifest, ReleaseTarget, SiteSpec};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledConfigBundle {
    pub manifest: ReleaseManifest,
    pub rendered_config: Value,
    pub certificates: Vec<CertificateRef>,
    pub signature: String,
}

pub fn compile_site_bundle(
    site: SiteSpec,
    certificates: Vec<CertificateRef>,
    targets: Vec<ReleaseTarget>,
    release_version: String,
) -> Result<CompiledConfigBundle> {
    if site.domain.trim().is_empty() {
        return Err(anyhow!("site domain cannot be empty"));
    }

    let rendered_config = json!({
        "site_code": site.site_code,
        "status": site.status,
        "domain": site.domain,
        "listen_port": site.listen_port,
        "protocol": site.protocol,
        "tls_enabled": site.tls_enabled,
        "upstreams": site.upstreams,
        "routes": site.routes,
        "cache_rules": site.cache_rules,
    });

    let config_hash = format!("sha256:{}:{}", site.name, release_version);
    let manifest = ReleaseManifest {
        release_id: Uuid::new_v4(),
        release_version: release_version.clone(),
        scope: "site".to_string(),
        config_hash,
        created_at: Utc::now(),
        sites: vec![site],
        certificates: certificates.clone(),
        targets,
    };

    let signature = signature_for_release(&manifest.release_version, manifest.release_id);

    Ok(CompiledConfigBundle {
        manifest,
        rendered_config,
        certificates,
        signature,
    })
}

pub fn compile_site_cleanup_bundle(
    site: SiteSpec,
    targets: Vec<ReleaseTarget>,
    release_version: String,
) -> Result<CompiledConfigBundle> {
    if site.site_code.trim().is_empty() {
        return Err(anyhow!("site code cannot be empty"));
    }

    let rendered_config = json!({
        "sites": [],
        "cleanup": {
            "site_ids": [site.id],
            "site_codes": [site.site_code.clone()],
            "domains": [site.domain.clone()],
        },
    });

    let config_hash = format!("sha256:cleanup:{}:{}", site.site_code, release_version);
    let manifest = ReleaseManifest {
        release_id: Uuid::new_v4(),
        release_version: release_version.clone(),
        scope: "site_cleanup".to_string(),
        config_hash,
        created_at: Utc::now(),
        sites: Vec::new(),
        certificates: Vec::new(),
        targets,
    };

    let signature = signature_for_release(&manifest.release_version, manifest.release_id);

    Ok(CompiledConfigBundle {
        manifest,
        rendered_config,
        certificates: Vec::new(),
        signature,
    })
}

fn signature_for_release(release_version: &str, release_id: Uuid) -> String {
    format!("sig:{release_version}:{release_id}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pingorahub_domain::{
        Protocol, RouteMatchType, SiteRoute, SiteStatus, Upstream, UpstreamBalanceMethod,
        UpstreamEndpoint,
    };

    #[test]
    fn compile_site_bundle_includes_routes() {
        let site = SiteSpec {
            id: Uuid::new_v4(),
            site_code: "route-demo".to_string(),
            name: "Route Demo".to_string(),
            domain: "route.example.com".to_string(),
            listen_port: 80,
            protocol: Protocol::Http,
            tls_enabled: false,
            status: SiteStatus::Published,
            upstreams: vec![
                Upstream {
                    name: "web".to_string(),
                    balance_method: UpstreamBalanceMethod::RoundRobin,
                    endpoints: vec![UpstreamEndpoint {
                        address: "10.20.3.65:8088".to_string(),
                        weight: 100,
                        active: true,
                        backup: false,
                    }],
                },
                Upstream {
                    name: "api".to_string(),
                    balance_method: UpstreamBalanceMethod::WeightedRoundRobin,
                    endpoints: vec![UpstreamEndpoint {
                        address: "10.20.3.68:9000".to_string(),
                        weight: 100,
                        active: true,
                        backup: false,
                    }],
                },
            ],
            routes: vec![
                SiteRoute {
                    name: "api-route".to_string(),
                    enabled: true,
                    match_type: RouteMatchType::PathPrefix,
                    path: "/api".to_string(),
                    upstream: "api".to_string(),
                    priority: 10,
                    strip_prefix: false,
                },
                SiteRoute {
                    name: "default".to_string(),
                    enabled: true,
                    match_type: RouteMatchType::PathPrefix,
                    path: "/".to_string(),
                    upstream: "web".to_string(),
                    priority: 1000,
                    strip_prefix: false,
                },
            ],
            cache_rules: Vec::new(),
        };

        let bundle = compile_site_bundle(site, Vec::new(), Vec::new(), "rel-route-001".to_string())
            .expect("site bundle should compile");

        assert_eq!(bundle.rendered_config["routes"][0]["upstream"], "api");
        assert_eq!(bundle.rendered_config["routes"][1]["path"], "/");
        assert_eq!(bundle.manifest.sites[0].routes[0].path, "/api");
    }
}
