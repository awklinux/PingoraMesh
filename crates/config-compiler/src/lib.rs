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
