use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use openssl::{
    bn::{BigNum, BigNumContext},
    ecdsa::EcdsaSig,
    hash::MessageDigest,
    nid::Nid,
    pkey::{Id, PKey, Private},
    sign::Signer,
    stack::Stack,
    symm::Cipher,
    x509::{X509, X509NameRef, X509ReqBuilder, extension::SubjectAlternativeName},
};
use pingorahub_dns_provider::{AcmeChallenge, DnsProvider, ProviderConfig, build_provider};
use reqwest::{
    Client, Response,
    header::{HeaderValue, LOCATION},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use std::{collections::HashMap, fs, time::Duration as StdDuration};
use tokio::{
    sync::Mutex,
    time::{Instant, sleep},
};
use uuid::Uuid;

const ACTIVE_RENEWAL_ORDER_STATUSES: &[&str] = &[
    "pending_dns_challenge",
    "dns_challenge_presenting",
    "dns_challenge_presented",
    "issuing",
];

#[derive(Debug, Clone)]
pub struct QueuedDnsChallengeOrder {
    pub order_id: Uuid,
    pub site_id: Option<Uuid>,
    pub certificate_id: Uuid,
    pub order_type: String,
    pub acme_provider: String,
    pub provider_type: String,
    pub api_endpoint: Option<String>,
    pub credentials: String,
    pub zone_name: String,
    pub external_zone_id: String,
    pub challenge_payload: Value,
    pub common_name: String,
    pub sans: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct QueuedCertificateIssuanceOrder {
    pub order_id: Uuid,
    pub site_id: Option<Uuid>,
    pub certificate_id: Uuid,
    pub order_type: String,
    pub acme_provider: String,
    pub provider_type: String,
    pub api_endpoint: Option<String>,
    pub credentials: String,
    pub zone_name: String,
    pub external_zone_id: String,
    pub challenge_payload: Value,
    pub common_name: String,
    pub sans: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PreparedDns01Challenge {
    pub challenge_payload: Value,
}

#[derive(Debug, Clone)]
pub struct IssuedCertificateMaterial {
    pub issuer: String,
    pub not_before: DateTime<Utc>,
    pub not_after: DateTime<Utc>,
    pub cert_pem: String,
    pub key_pem_encrypted: String,
    pub chain_pem: Option<String>,
    pub fingerprint_sha256: String,
}

#[derive(Debug, Clone)]
pub struct PersistIssuedCertificateRequest {
    pub order_id: Uuid,
    pub site_id: Option<Uuid>,
    pub certificate_id: Uuid,
    pub order_type: String,
    pub warning_message: Option<String>,
    pub material: IssuedCertificateMaterial,
}

#[async_trait]
pub trait CertificateOrderStore: Send + Sync {
    async fn claim_pending_dns_orders(&self, limit: usize) -> Result<Vec<QueuedDnsChallengeOrder>>;
    async fn update_order_challenge_payload(&self, order_id: Uuid, payload: &Value) -> Result<()>;
    async fn claim_ready_issuance_orders(
        &self,
        limit: usize,
    ) -> Result<Vec<QueuedCertificateIssuanceOrder>>;
    async fn update_order_status(
        &self,
        order_id: Uuid,
        status: &str,
        error_message: Option<String>,
    ) -> Result<()>;
    async fn persist_issued_certificate(
        &self,
        request: &PersistIssuedCertificateRequest,
    ) -> Result<()>;
}

#[async_trait]
pub trait AcmeClient: Send + Sync {
    async fn prepare_dns01_challenge(
        &self,
        order: &QueuedDnsChallengeOrder,
    ) -> Result<PreparedDns01Challenge>;
    async fn acknowledge_dns01_challenge(&self, challenge_payload: &Value) -> Result<()>;
    async fn finalize_dns01_order(
        &self,
        order: &QueuedCertificateIssuanceOrder,
    ) -> Result<IssuedCertificateMaterial>;
}

#[derive(Debug, Clone, Default)]
pub struct MockAcmeClient;

#[async_trait]
impl AcmeClient for MockAcmeClient {
    async fn prepare_dns01_challenge(
        &self,
        order: &QueuedDnsChallengeOrder,
    ) -> Result<PreparedDns01Challenge> {
        let payload = if has_prepared_dns_payload(&order.challenge_payload) {
            order.challenge_payload.clone()
        } else {
            json!({
                "zone_name": order.zone_name,
                "identifier": order.common_name,
                "record_name": dns01_record_name(&order.common_name),
                "record_type": "TXT",
                "record_value": format!("token-{}", Uuid::new_v4()),
                "ttl": 60,
            })
        };

        Ok(PreparedDns01Challenge {
            challenge_payload: payload,
        })
    }

    async fn acknowledge_dns01_challenge(&self, _challenge_payload: &Value) -> Result<()> {
        Ok(())
    }

    async fn finalize_dns01_order(
        &self,
        order: &QueuedCertificateIssuanceOrder,
    ) -> Result<IssuedCertificateMaterial> {
        let issued_at = Utc::now();
        let not_before = issued_at - Duration::minutes(5);
        let not_after = issued_at + Duration::days(90);
        let sans = if order.sans.is_empty() {
            vec![order.common_name.clone()]
        } else {
            order.sans.clone()
        };
        let issuer = format!("PingoraHub Mock ACME ({})", order.acme_provider);
        let certificate_payload = format!(
            "mock-certificate\norder_id={}\nsite_id={}\ncommon_name={}\nsans={}\nissuer={}\nissued_at={}",
            order.order_id,
            order
                .site_id
                .map(|site_id| site_id.to_string())
                .unwrap_or_else(|| "standalone".to_string()),
            order.common_name,
            sans.join(","),
            issuer,
            issued_at.to_rfc3339(),
        );
        let private_key_payload = format!(
            "mock-private-key\norder_id={}\nissued_at={}",
            order.order_id,
            issued_at.to_rfc3339(),
        );
        let chain_payload = format!(
            "mock-chain\nprovider={}\ncommon_name={}",
            order.acme_provider, order.common_name
        );
        let cert_pem = to_mock_pem_block("CERTIFICATE", &certificate_payload);
        let key_pem_encrypted = to_mock_pem_block("ENCRYPTED PRIVATE KEY", &private_key_payload);
        let chain_pem = Some(to_mock_pem_block("CERTIFICATE", &chain_payload));
        let fingerprint_sha256 = sha256_hex(cert_pem.as_bytes());

        Ok(IssuedCertificateMaterial {
            issuer,
            not_before,
            not_after,
            cert_pem,
            key_pem_encrypted,
            chain_pem,
            fingerprint_sha256,
        })
    }
}

#[derive(Debug, Clone)]
struct RealAcmeConfig {
    directory_url: String,
    contact_email: Option<String>,
    account_key_pem: String,
    account_key_passphrase: Option<String>,
    cert_key_passphrase: String,
    dns_propagation_wait_seconds: u64,
    poll_interval_seconds: u64,
    poll_timeout_seconds: u64,
}

impl RealAcmeConfig {
    fn from_env(default_directory_url: &str) -> Result<Self> {
        let directory_url = std::env::var("PINGORAHUB_ACME_DIRECTORY_URL")
            .unwrap_or_else(|_| default_directory_url.to_string());
        let contact_email = std::env::var("PINGORAHUB_ACME_CONTACT_EMAIL")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let account_key_passphrase = std::env::var("PINGORAHUB_ACME_ACCOUNT_KEY_PASSPHRASE")
            .ok()
            .filter(|value| !value.is_empty());
        let account_key_pem = if let Ok(path) =
            std::env::var("PINGORAHUB_ACME_ACCOUNT_KEY_PEM_PATH")
        {
            fs::read_to_string(&path)
                .with_context(|| format!("failed to read acme account key from {path}"))?
        } else if let Ok(inline) = std::env::var("PINGORAHUB_ACME_ACCOUNT_KEY_PEM") {
            inline
        } else {
            bail!(
                "real acme mode requires PINGORAHUB_ACME_ACCOUNT_KEY_PEM_PATH or PINGORAHUB_ACME_ACCOUNT_KEY_PEM"
            );
        };

        Ok(Self {
            directory_url,
            contact_email,
            account_key_pem,
            account_key_passphrase,
            cert_key_passphrase: std::env::var("PINGORAHUB_ACME_CERT_KEY_PASSPHRASE")
                .unwrap_or_else(|_| "pingorahub-dev-only".to_string()),
            dns_propagation_wait_seconds: std::env::var(
                "PINGORAHUB_ACME_DNS_PROPAGATION_WAIT_SECONDS",
            )
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(15),
            poll_interval_seconds: std::env::var("PINGORAHUB_ACME_POLL_INTERVAL_SECONDS")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(5),
            poll_timeout_seconds: std::env::var("PINGORAHUB_ACME_POLL_TIMEOUT_SECONDS")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(180),
        })
    }
}

pub struct LetsEncryptAcmeClient {
    client: Client,
    config: RealAcmeConfig,
    directory_cache: Mutex<Option<AcmeDirectory>>,
    account_url_cache: Mutex<Option<String>>,
}

impl LetsEncryptAcmeClient {
    fn new(config: RealAcmeConfig) -> Result<Self> {
        Ok(Self {
            client: Client::builder()
                .user_agent("PingoraHub/0.1 (+acme)")
                .build()
                .context("failed to build acme http client")?,
            config,
            directory_cache: Mutex::new(None),
            account_url_cache: Mutex::new(None),
        })
    }

    async fn directory(&self) -> Result<AcmeDirectory> {
        if let Some(directory) = self.directory_cache.lock().await.clone() {
            return Ok(directory);
        }

        let directory = self
            .client
            .get(&self.config.directory_url)
            .send()
            .await
            .context("failed to fetch acme directory")?;
        let directory =
            parse_json_response::<AcmeDirectory>(directory, "fetch acme directory").await?;
        *self.directory_cache.lock().await = Some(directory.clone());
        Ok(directory)
    }

    async fn account_url(&self) -> Result<String> {
        if let Some(url) = self.account_url_cache.lock().await.clone() {
            return Ok(url);
        }

        let directory = self.directory().await?;
        let payload = if let Some(contact_email) = &self.config.contact_email {
            json!({
                "termsOfServiceAgreed": true,
                "contact": [format!("mailto:{contact_email}")]
            })
        } else {
            json!({
                "termsOfServiceAgreed": true
            })
        };

        let response = self
            .signed_post(&directory.new_account, Some(payload), JwsIdentity::Jwk)
            .await
            .context("failed to create or fetch acme account")?;
        let response = ensure_success(
            response.status().is_success(),
            response,
            "create acme account",
        )
        .await?;
        let account_url = location_header(response.headers())
            .context("acme newAccount response missing Location header")?;
        *self.account_url_cache.lock().await = Some(account_url.clone());
        Ok(account_url)
    }

    async fn new_nonce(&self) -> Result<String> {
        let directory = self.directory().await?;
        let response = self
            .client
            .head(&directory.new_nonce)
            .send()
            .await
            .context("failed to fetch acme replay nonce")?;
        replay_nonce(response.headers())
    }

    fn account_key(&self) -> Result<PKey<Private>> {
        if let Some(passphrase) = &self.config.account_key_passphrase {
            PKey::private_key_from_pem_passphrase(
                self.config.account_key_pem.as_bytes(),
                passphrase.as_bytes(),
            )
            .context("failed to load encrypted acme account key")
        } else {
            PKey::private_key_from_pem(self.config.account_key_pem.as_bytes())
                .context("failed to load acme account key")
        }
    }

    fn account_jwk(&self) -> Result<EcJwk> {
        let key = self.account_key()?;
        if key.id() != Id::EC {
            bail!("acme account key must be an EC P-256 private key");
        }

        let ec_key = key.ec_key().context("failed to read acme ec account key")?;
        let group = ec_key.group();
        if group.curve_name() != Some(Nid::X9_62_PRIME256V1) {
            bail!("acme account key must use the P-256 curve");
        }

        let mut x = BigNum::new()?;
        let mut y = BigNum::new()?;
        let mut ctx = BigNumContext::new()?;
        ec_key
            .public_key()
            .affine_coordinates_gfp(group, &mut x, &mut y, &mut ctx)
            .context("failed to extract acme account public key coordinates")?;

        Ok(EcJwk {
            crv: "P-256".to_string(),
            kty: "EC".to_string(),
            x: URL_SAFE_NO_PAD.encode(pad_big_num(&x, 32)),
            y: URL_SAFE_NO_PAD.encode(pad_big_num(&y, 32)),
        })
    }

    fn jwk_thumbprint(&self) -> Result<String> {
        let jwk = self.account_jwk()?;
        let digest = Sha256::digest(
            serde_json::to_string(&jwk)
                .context("failed to serialize acme jwk for thumbprint")?
                .as_bytes(),
        );
        Ok(URL_SAFE_NO_PAD.encode(digest))
    }

    async fn signed_post(
        &self,
        url: &str,
        payload: Option<Value>,
        identity: JwsIdentity<'_>,
    ) -> Result<Response> {
        let nonce = self.new_nonce().await?;
        let protected = match identity {
            JwsIdentity::Jwk => json!({
                "alg": "ES256",
                "nonce": nonce,
                "url": url,
                "jwk": self.account_jwk()?,
            }),
            JwsIdentity::Kid(kid) => json!({
                "alg": "ES256",
                "nonce": nonce,
                "url": url,
                "kid": kid,
            }),
        };
        let protected_b64 = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&protected).context("failed to serialize acme protected header")?,
        );
        let payload_b64 = match payload {
            Some(payload) => URL_SAFE_NO_PAD
                .encode(serde_json::to_vec(&payload).context("failed to serialize acme payload")?),
            None => String::new(),
        };
        let signature = self.sign_compact_jws(&protected_b64, &payload_b64)?;
        let body = json!({
            "protected": protected_b64,
            "payload": payload_b64,
            "signature": signature,
        });

        self.client
            .post(url)
            .header("Content-Type", "application/jose+json")
            .json(&body)
            .send()
            .await
            .with_context(|| format!("failed to call acme endpoint {url}"))
    }

    fn sign_compact_jws(&self, protected_b64: &str, payload_b64: &str) -> Result<String> {
        let input = format!("{protected_b64}.{payload_b64}");
        let key = self.account_key()?;
        let mut signer =
            Signer::new(MessageDigest::sha256(), &key).context("failed to create acme signer")?;
        signer
            .update(input.as_bytes())
            .context("failed to feed acme signer input")?;
        let der_signature = signer
            .sign_to_vec()
            .context("failed to sign acme request")?;
        let signature =
            EcdsaSig::from_der(&der_signature).context("failed to parse acme signature der")?;
        let raw_signature = [
            pad_big_num(signature.r(), 32),
            pad_big_num(signature.s(), 32),
        ]
        .concat();
        Ok(URL_SAFE_NO_PAD.encode(raw_signature))
    }

    async fn post_as_get_json<T: DeserializeOwned>(&self, url: &str) -> Result<T> {
        let account_url = self.account_url().await?;
        let response = self
            .signed_post(url, None, JwsIdentity::Kid(&account_url))
            .await
            .with_context(|| format!("failed acme POST-as-GET for {url}"))?;
        parse_json_response(response, "acme POST-as-GET").await
    }

    async fn post_as_get_text(&self, url: &str) -> Result<String> {
        let account_url = self.account_url().await?;
        let response = self
            .signed_post(url, None, JwsIdentity::Kid(&account_url))
            .await
            .with_context(|| format!("failed acme POST-as-GET text for {url}"))?;
        parse_text_response(response, "acme POST-as-GET text").await
    }

    async fn poll_authorization_valid(&self, url: &str) -> Result<()> {
        let deadline =
            Instant::now() + StdDuration::from_secs(self.config.poll_timeout_seconds.max(1));
        loop {
            let authorization: AcmeAuthorization = self.post_as_get_json(url).await?;
            match authorization.status.as_str() {
                "valid" => return Ok(()),
                "pending" | "processing" => {}
                "invalid" => {
                    let detail = authorization
                        .challenges
                        .into_iter()
                        .find_map(|challenge| challenge.error.and_then(|error| error.detail));
                    bail!(
                        "acme authorization became invalid{}",
                        detail.map(|value| format!(": {value}")).unwrap_or_default()
                    );
                }
                other => bail!("unexpected acme authorization status: {other}"),
            }

            if Instant::now() >= deadline {
                bail!("timed out waiting for acme authorization to become valid");
            }
            sleep(StdDuration::from_secs(
                self.config.poll_interval_seconds.max(1),
            ))
            .await;
        }
    }

    async fn finalize_and_download_certificate(
        &self,
        challenge_payload: &Value,
        common_name: &str,
        sans: &[String],
    ) -> Result<IssuedCertificateMaterial> {
        let finalize_url = payload_str(challenge_payload, "finalize_url")?;
        let order_url = payload_str(challenge_payload, "order_url")?;
        let csr = generate_certificate_csr(
            common_name,
            sans,
            self.config.cert_key_passphrase.as_bytes(),
        )?;
        let account_url = self.account_url().await?;
        let response = self
            .signed_post(
                &finalize_url,
                Some(json!({
                    "csr": URL_SAFE_NO_PAD.encode(&csr.csr_der),
                })),
                JwsIdentity::Kid(&account_url),
            )
            .await
            .context("failed to finalize acme order")?;
        ensure_success(
            response.status().is_success(),
            response,
            "finalize acme order",
        )
        .await?;

        let deadline =
            Instant::now() + StdDuration::from_secs(self.config.poll_timeout_seconds.max(1));
        loop {
            let order: AcmeOrder = self.post_as_get_json(&order_url).await?;
            match order.status.as_str() {
                "valid" => {
                    let certificate_url = order
                        .certificate
                        .ok_or_else(|| anyhow!("acme order missing certificate url"))?;
                    let chain_pem = self.post_as_get_text(&certificate_url).await?;
                    return issued_material_from_chain(&chain_pem, csr.key_pem_encrypted);
                }
                "ready" | "processing" | "pending" => {}
                "invalid" => {
                    let detail = order.error.and_then(|error| error.detail);
                    bail!(
                        "acme order became invalid{}",
                        detail.map(|value| format!(": {value}")).unwrap_or_default()
                    );
                }
                other => bail!("unexpected acme order status: {other}"),
            }

            if Instant::now() >= deadline {
                bail!("timed out waiting for acme order to become valid");
            }
            sleep(StdDuration::from_secs(
                self.config.poll_interval_seconds.max(1),
            ))
            .await;
        }
    }
}

#[async_trait]
impl AcmeClient for LetsEncryptAcmeClient {
    async fn prepare_dns01_challenge(
        &self,
        order: &QueuedDnsChallengeOrder,
    ) -> Result<PreparedDns01Challenge> {
        if has_prepared_dns_payload(&order.challenge_payload) {
            return Ok(PreparedDns01Challenge {
                challenge_payload: order.challenge_payload.clone(),
            });
        }

        let identifiers = unique_identifiers(&order.common_name, &order.sans);
        if identifiers.len() != 1 {
            bail!("real acme mode currently supports a single dns identifier per order");
        }

        let directory = self.directory().await?;
        let account_url = self.account_url().await?;
        let order_response = self
            .signed_post(
                &directory.new_order,
                Some(json!({
                    "identifiers": identifiers
                        .iter()
                        .map(|value| json!({ "type": "dns", "value": value }))
                        .collect::<Vec<_>>(),
                })),
                JwsIdentity::Kid(&account_url),
            )
            .await
            .context("failed to create acme order")?;
        let order_url = location_header(order_response.headers())
            .context("acme newOrder response missing Location header")?;
        let order_body =
            parse_json_response::<AcmeOrder>(order_response, "create acme order").await?;
        let authorization_url = order_body
            .authorizations
            .first()
            .cloned()
            .ok_or_else(|| anyhow!("acme order missing authorization url"))?;
        let authorization: AcmeAuthorization = self.post_as_get_json(&authorization_url).await?;
        let dns_challenge = authorization
            .challenges
            .into_iter()
            .find(|challenge| challenge.kind == "dns-01")
            .ok_or_else(|| anyhow!("acme authorization missing dns-01 challenge"))?;
        let token = dns_challenge
            .token
            .ok_or_else(|| anyhow!("acme dns-01 challenge missing token"))?;
        let key_authorization = format!("{token}.{}", self.jwk_thumbprint()?);
        let record_value = URL_SAFE_NO_PAD.encode(Sha256::digest(key_authorization.as_bytes()));
        let identifier = authorization.identifier.value;
        let challenge_payload = json!({
            "zone_name": order.zone_name,
            "record_name": dns01_record_name(&identifier),
            "record_type": "TXT",
            "record_value": record_value,
            "ttl": 60,
            "identifier": identifier,
            "order_url": order_url,
            "authorization_url": authorization_url,
            "challenge_url": dns_challenge.url,
            "challenge_token": token,
            "finalize_url": order_body.finalize,
        });

        Ok(PreparedDns01Challenge { challenge_payload })
    }

    async fn acknowledge_dns01_challenge(&self, challenge_payload: &Value) -> Result<()> {
        let challenge_url = payload_str(challenge_payload, "challenge_url")?;
        if self.config.dns_propagation_wait_seconds > 0 {
            sleep(StdDuration::from_secs(
                self.config.dns_propagation_wait_seconds,
            ))
            .await;
        }
        let account_url = self.account_url().await?;
        let response = self
            .signed_post(
                &challenge_url,
                Some(json!({})),
                JwsIdentity::Kid(&account_url),
            )
            .await
            .context("failed to acknowledge acme dns-01 challenge")?;
        ensure_success(
            response.status().is_success(),
            response,
            "acknowledge acme dns-01 challenge",
        )
        .await?;
        Ok(())
    }

    async fn finalize_dns01_order(
        &self,
        order: &QueuedCertificateIssuanceOrder,
    ) -> Result<IssuedCertificateMaterial> {
        let authorization_url = payload_str(&order.challenge_payload, "authorization_url")?;
        self.poll_authorization_valid(&authorization_url).await?;
        self.finalize_and_download_certificate(
            &order.challenge_payload,
            &order.common_name,
            &order.sans,
        )
        .await
    }
}

pub fn build_acme_client(mode: &str) -> Result<Box<dyn AcmeClient>> {
    match mode {
        "mock" => Ok(Box::new(MockAcmeClient)),
        "letsencrypt-staging" => Ok(Box::new(LetsEncryptAcmeClient::new(
            RealAcmeConfig::from_env("https://acme-staging-v02.api.letsencrypt.org/directory")?,
        )?)),
        "letsencrypt-production" => Ok(Box::new(LetsEncryptAcmeClient::new(
            RealAcmeConfig::from_env("https://acme-v02.api.letsencrypt.org/directory")?,
        )?)),
        "custom" => Ok(Box::new(LetsEncryptAcmeClient::new(
            RealAcmeConfig::from_env("https://acme-staging-v02.api.letsencrypt.org/directory")?,
        )?)),
        other => anyhow::bail!("unsupported acme mode: {other}"),
    }
}

#[derive(Debug, Clone)]
pub struct PostgresCertificateOrderStore {
    pool: PgPool,
}

impl PostgresCertificateOrderStore {
    pub async fn connect(url: &str, max_connections: u32) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .connect(url)
            .await
            .context("failed to connect certificate order postgres store")?;
        Ok(Self { pool })
    }

    pub async fn enqueue_due_renewal_orders(
        &self,
        now: DateTime<Utc>,
        renew_before: Duration,
        limit: usize,
        acme_provider: &str,
    ) -> Result<usize> {
        let threshold = now + renew_before;
        let active_renewal_statuses = ACTIVE_RENEWAL_ORDER_STATUSES
            .iter()
            .map(|status| (*status).to_string())
            .collect::<Vec<_>>();
        debug_assert!(
            active_renewal_statuses
                .iter()
                .all(|status| is_active_renewal_order_status(status))
        );
        let certificates = sqlx::query(
            r#"
            SELECT c.id, c.common_name, c.sans
            FROM certificates c
            WHERE
                c.status = 'active'
                AND c.not_after IS NOT NULL
                AND c.not_after <= $1
                AND NOT EXISTS (
                    SELECT 1
                    FROM certificate_orders o
                    WHERE
                        o.certificate_id = c.id
                        AND o.order_type = 'renew'
                        AND o.order_status = ANY($3)
                )
            ORDER BY c.not_after ASC
            LIMIT $2
            "#,
        )
        .bind(threshold)
        .bind(i64::try_from(limit).unwrap_or(i64::MAX))
        .bind(&active_renewal_statuses)
        .fetch_all(&self.pool)
        .await?;
        if certificates.is_empty() {
            return Ok(0);
        }

        let zones = sqlx::query(
            r#"
            SELECT id, zone_name
            FROM dns_zones
            WHERE status = 'active'
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut created = 0usize;
        for certificate in certificates {
            let certificate_id: Uuid = certificate.get("id");
            let common_name: String = certificate.get("common_name");
            let sans: Vec<String> = serde_json::from_value(certificate.get("sans"))
                .context("failed to decode certificate sans json")?;
            let Some(zone) = zones
                .iter()
                .filter(|zone| {
                    let zone_name: String = zone.get("zone_name");
                    domain_matches_zone(&common_name, &zone_name)
                })
                .max_by_key(|zone| {
                    let zone_name: String = zone.get("zone_name");
                    zone_name.len()
                })
            else {
                continue;
            };

            let zone_id: Uuid = zone.get("id");
            let zone_name: String = zone.get("zone_name");
            let challenge_payload = json!({
                "zone_name": zone_name,
                "identifier": common_name,
                "sans": sans,
                "record_name": dns01_record_name(&common_name),
                "record_type": "TXT",
                "record_value": format!("token-{}", Uuid::new_v4()),
                "ttl": 60,
                "auto_renew": true,
            });

            let result = sqlx::query(
                r#"
                INSERT INTO certificate_orders (
                    id, site_id, certificate_id, zone_id, order_type, acme_provider,
                    challenge_type, challenge_payload, order_status, created_at, updated_at
                )
                VALUES ($1, NULL, $2, $3, 'renew', $4, 'dns-01', $5, 'pending_dns_challenge', now(), now())
                ON CONFLICT DO NOTHING
                "#,
            )
            .bind(Uuid::new_v4())
            .bind(certificate_id)
            .bind(zone_id)
            .bind(acme_provider)
            .bind(&challenge_payload)
            .execute(&self.pool)
            .await?;
            if result.rows_affected() > 0 {
                created += 1;
            }
        }

        Ok(created)
    }
}

#[async_trait]
impl CertificateOrderStore for PostgresCertificateOrderStore {
    async fn claim_pending_dns_orders(&self, limit: usize) -> Result<Vec<QueuedDnsChallengeOrder>> {
        let mut tx = self.pool.begin().await?;
        let rows = sqlx::query(
            r#"
            SELECT
                o.id AS order_id,
                o.site_id,
                o.certificate_id,
                o.order_type,
                o.acme_provider,
                o.challenge_payload,
                p.provider_type::text AS provider_type,
                p.api_endpoint,
                p.credential_encrypted,
                z.zone_name,
                z.external_zone_id,
                c.common_name,
                c.sans
            FROM certificate_orders o
            INNER JOIN dns_zones z ON z.id = o.zone_id
            INNER JOIN dns_providers p ON p.id = z.provider_id
            INNER JOIN certificates c ON c.id = o.certificate_id
            WHERE
                o.order_status = 'pending_dns_challenge'
                AND o.challenge_type = 'dns-01'
                AND o.certificate_id IS NOT NULL
            ORDER BY o.created_at ASC
            FOR UPDATE SKIP LOCKED
            LIMIT $1
            "#,
        )
        .bind(i64::try_from(limit).unwrap_or(i64::MAX))
        .fetch_all(&mut *tx)
        .await?;

        let mut jobs = Vec::with_capacity(rows.len());
        for row in rows {
            let order_id: Uuid = row.get("order_id");
            sqlx::query(
                r#"
                UPDATE certificate_orders
                SET order_status = 'dns_challenge_presenting', error_message = NULL, updated_at = now()
                WHERE id = $1
                "#,
            )
            .bind(order_id)
            .execute(&mut *tx)
            .await?;

            jobs.push(QueuedDnsChallengeOrder {
                order_id,
                site_id: row.get("site_id"),
                certificate_id: row.get("certificate_id"),
                order_type: row.get("order_type"),
                acme_provider: row.get("acme_provider"),
                provider_type: row.get("provider_type"),
                api_endpoint: row.get("api_endpoint"),
                credentials: row.get("credential_encrypted"),
                zone_name: row.get("zone_name"),
                external_zone_id: row.get("external_zone_id"),
                challenge_payload: row.get("challenge_payload"),
                common_name: row.get("common_name"),
                sans: serde_json::from_value(row.get("sans"))
                    .context("failed to decode certificate sans json")?,
            });
        }

        tx.commit().await?;
        Ok(jobs)
    }

    async fn update_order_challenge_payload(&self, order_id: Uuid, payload: &Value) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE certificate_orders
            SET challenge_payload = $2, updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(order_id)
        .bind(payload)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn claim_ready_issuance_orders(
        &self,
        limit: usize,
    ) -> Result<Vec<QueuedCertificateIssuanceOrder>> {
        let mut tx = self.pool.begin().await?;
        let rows = sqlx::query(
            r#"
            SELECT
                o.id AS order_id,
                o.site_id,
                o.certificate_id,
                o.order_type,
                o.acme_provider,
                o.challenge_payload,
                p.provider_type::text AS provider_type,
                p.api_endpoint,
                p.credential_encrypted,
                z.zone_name,
                z.external_zone_id,
                c.common_name,
                c.sans
            FROM certificate_orders o
            INNER JOIN dns_zones z ON z.id = o.zone_id
            INNER JOIN dns_providers p ON p.id = z.provider_id
            INNER JOIN certificates c ON c.id = o.certificate_id
            WHERE
                o.order_status = 'dns_challenge_presented'
                AND o.challenge_type = 'dns-01'
                AND o.certificate_id IS NOT NULL
            ORDER BY o.created_at ASC
            FOR UPDATE SKIP LOCKED
            LIMIT $1
            "#,
        )
        .bind(i64::try_from(limit).unwrap_or(i64::MAX))
        .fetch_all(&mut *tx)
        .await?;

        let mut jobs = Vec::with_capacity(rows.len());
        for row in rows {
            let order_id: Uuid = row.get("order_id");
            sqlx::query(
                r#"
                UPDATE certificate_orders
                SET order_status = 'issuing', error_message = NULL, updated_at = now()
                WHERE id = $1
                "#,
            )
            .bind(order_id)
            .execute(&mut *tx)
            .await?;

            jobs.push(QueuedCertificateIssuanceOrder {
                order_id,
                site_id: row.get("site_id"),
                certificate_id: row.get("certificate_id"),
                order_type: row.get("order_type"),
                acme_provider: row.get("acme_provider"),
                provider_type: row.get("provider_type"),
                api_endpoint: row.get("api_endpoint"),
                credentials: row.get("credential_encrypted"),
                zone_name: row.get("zone_name"),
                external_zone_id: row.get("external_zone_id"),
                challenge_payload: row.get("challenge_payload"),
                common_name: row.get("common_name"),
                sans: serde_json::from_value(row.get("sans"))
                    .context("failed to decode certificate sans json")?,
            });
        }

        tx.commit().await?;
        Ok(jobs)
    }

    async fn update_order_status(
        &self,
        order_id: Uuid,
        status: &str,
        error_message: Option<String>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE certificate_orders
            SET order_status = $2, error_message = $3, updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(order_id)
        .bind(status)
        .bind(error_message.as_deref())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn persist_issued_certificate(
        &self,
        request: &PersistIssuedCertificateRequest,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        let version = sqlx::query_scalar::<_, i32>(
            r#"
            SELECT COALESCE(MAX(version), 0) + 1
            FROM certificate_versions
            WHERE certificate_id = $1
            "#,
        )
        .bind(request.certificate_id)
        .fetch_one(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            UPDATE certificates
            SET
                issuer = $2,
                not_before = $3,
                not_after = $4,
                fingerprint_sha256 = $5,
                status = 'active',
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(request.certificate_id)
        .bind(&request.material.issuer)
        .bind(request.material.not_before)
        .bind(request.material.not_after)
        .bind(&request.material.fingerprint_sha256)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO certificate_versions (
                id, certificate_id, version, cert_pem, key_pem_encrypted, chain_pem, kms_key_id, created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, now())
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(request.certificate_id)
        .bind(version)
        .bind(&request.material.cert_pem)
        .bind(&request.material.key_pem_encrypted)
        .bind(request.material.chain_pem.as_deref())
        .bind(Option::<&str>::None)
        .execute(&mut *tx)
        .await?;

        let target_site_ids = if let Some(site_id) = request.site_id {
            vec![site_id]
        } else if request.order_type == "renew" {
            sqlx::query_scalar::<_, Uuid>(
                r#"
                SELECT DISTINCT site_id
                FROM site_cert_bindings
                WHERE certificate_id = $1 AND is_default = TRUE
                "#,
            )
            .bind(request.certificate_id)
            .fetch_all(&mut *tx)
            .await?
        } else {
            Vec::new()
        };

        for site_id in target_site_ids {
            sqlx::query("UPDATE site_cert_bindings SET is_default = FALSE WHERE site_id = $1")
                .bind(site_id)
                .execute(&mut *tx)
                .await?;

            sqlx::query(
                r#"
                INSERT INTO site_cert_bindings (id, site_id, certificate_id, version, is_default, created_at)
                VALUES ($1, $2, $3, $4, TRUE, now())
                "#,
            )
            .bind(Uuid::new_v4())
            .bind(site_id)
            .bind(request.certificate_id)
            .bind(version)
            .execute(&mut *tx)
            .await?;
        }

        sqlx::query(
            r#"
            UPDATE certificate_orders
            SET order_status = 'issued', error_message = $2, updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(request.order_id)
        .bind(request.warning_message.as_deref())
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }
}

pub async fn process_pending_certificate_orders(
    store: &(dyn CertificateOrderStore + Send + Sync),
    acme_client: &(dyn AcmeClient + Send + Sync),
    limit: usize,
) -> Result<usize> {
    let jobs = store.claim_pending_dns_orders(limit).await?;
    for job in &jobs {
        match process_dns_challenge(job, acme_client).await {
            Ok(prepared) => {
                store
                    .update_order_challenge_payload(job.order_id, &prepared.challenge_payload)
                    .await?;
                store
                    .update_order_status(job.order_id, "dns_challenge_presented", None)
                    .await?;
            }
            Err(error) => {
                store
                    .update_order_status(
                        job.order_id,
                        "dns_challenge_failed",
                        Some(error.to_string()),
                    )
                    .await?;
            }
        }
    }

    Ok(jobs.len())
}

pub async fn process_presented_certificate_orders(
    store: &(dyn CertificateOrderStore + Send + Sync),
    acme_client: &(dyn AcmeClient + Send + Sync),
    limit: usize,
) -> Result<usize> {
    let jobs = store.claim_ready_issuance_orders(limit).await?;
    for job in &jobs {
        match finalize_certificate_order(job, acme_client).await {
            Ok(result) => store.persist_issued_certificate(&result).await?,
            Err(error) => {
                store
                    .update_order_status(job.order_id, "issue_failed", Some(error.to_string()))
                    .await?;
            }
        }
    }

    Ok(jobs.len())
}

async fn process_dns_challenge(
    job: &QueuedDnsChallengeOrder,
    acme_client: &(dyn AcmeClient + Send + Sync),
) -> Result<PreparedDns01Challenge> {
    let prepared = acme_client.prepare_dns01_challenge(job).await?;
    let provider = provider_from_config(
        &job.provider_type,
        job.api_endpoint.clone(),
        &job.credentials,
        job.order_id,
    )?;
    let challenge = challenge_from_payload(
        job.order_id,
        &job.zone_name,
        &job.external_zone_id,
        &prepared.challenge_payload,
    )?;

    provider.present_dns01_challenge(&challenge).await?;
    acme_client
        .acknowledge_dns01_challenge(&prepared.challenge_payload)
        .await?;
    Ok(prepared)
}

async fn finalize_certificate_order(
    job: &QueuedCertificateIssuanceOrder,
    acme_client: &(dyn AcmeClient + Send + Sync),
) -> Result<PersistIssuedCertificateRequest> {
    let provider = provider_from_config(
        &job.provider_type,
        job.api_endpoint.clone(),
        &job.credentials,
        job.order_id,
    )?;
    let challenge = challenge_from_payload(
        job.order_id,
        &job.zone_name,
        &job.external_zone_id,
        &job.challenge_payload,
    )?;
    let material = acme_client.finalize_dns01_order(job).await?;
    let warning_message = match provider.cleanup_dns01_challenge(&challenge).await {
        Ok(()) => None,
        Err(error) => Some(format!(
            "certificate issued but dns challenge cleanup failed: {error}"
        )),
    };

    Ok(PersistIssuedCertificateRequest {
        order_id: job.order_id,
        site_id: job.site_id,
        certificate_id: job.certificate_id,
        order_type: job.order_type.clone(),
        warning_message,
        material,
    })
}

fn provider_from_config(
    provider_type: &str,
    api_endpoint: Option<String>,
    credentials: &str,
    order_id: Uuid,
) -> Result<Box<dyn DnsProvider>> {
    let credentials = serde_json::from_str(credentials)
        .with_context(|| format!("invalid dns provider credentials for order {order_id}"))?;
    build_provider(&ProviderConfig {
        provider_type: provider_type.to_string(),
        api_endpoint,
        credentials,
    })
}

fn challenge_from_payload(
    order_id: Uuid,
    zone_name: &str,
    external_zone_id: &str,
    payload: &Value,
) -> Result<AcmeChallenge> {
    let payload: CertificateChallengePayload = serde_json::from_value(payload.clone())
        .with_context(|| format!("invalid challenge payload for order {order_id}"))?;
    let record_type = payload
        .record_type
        .as_deref()
        .unwrap_or("TXT")
        .to_ascii_uppercase();
    if record_type != "TXT" {
        return Err(anyhow!(
            "unsupported dns challenge record type: {record_type}"
        ));
    }

    let fqdn = payload
        .record_name
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!("challenge payload record_name is required"))?;
    let value = payload
        .record_value
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!("challenge payload record_value is required"))?;

    Ok(AcmeChallenge {
        zone_id: Some(external_zone_id.to_string()),
        zone_name: payload.zone_name.unwrap_or_else(|| zone_name.to_string()),
        fqdn,
        value,
        ttl: payload.ttl.unwrap_or(60),
    })
}

fn to_mock_pem_block(label: &str, payload: &str) -> String {
    let encoded = STANDARD.encode(payload.as_bytes());
    let mut body = String::new();
    for chunk in encoded.as_bytes().chunks(64) {
        body.push_str(std::str::from_utf8(chunk).unwrap_or_default());
        body.push('\n');
    }
    format!("-----BEGIN {label}-----\n{body}-----END {label}-----\n")
}

fn sha256_hex(input: &[u8]) -> String {
    let digest = Sha256::digest(input);
    format!("{digest:x}")
}

fn unique_identifiers(common_name: &str, sans: &[String]) -> Vec<String> {
    let mut seen = HashMap::<String, ()>::new();
    let mut identifiers = Vec::new();
    for value in std::iter::once(common_name.to_string()).chain(sans.iter().cloned()) {
        if !value.trim().is_empty() && seen.insert(value.clone(), ()).is_none() {
            identifiers.push(value);
        }
    }
    identifiers
}

fn is_active_renewal_order_status(status: &str) -> bool {
    ACTIVE_RENEWAL_ORDER_STATUSES.contains(&status)
}

fn certificate_dns_domain(identifier: &str) -> &str {
    identifier.strip_prefix("*.").unwrap_or(identifier)
}

fn dns01_record_name(identifier: &str) -> String {
    format!("_acme-challenge.{}", certificate_dns_domain(identifier))
}

fn domain_matches_zone(identifier: &str, zone_name: &str) -> bool {
    let domain = certificate_dns_domain(identifier)
        .trim_end_matches('.')
        .to_ascii_lowercase();
    let zone_name = zone_name.trim().trim_end_matches('.').to_ascii_lowercase();
    domain == zone_name || domain.ends_with(&format!(".{zone_name}"))
}

fn payload_str(payload: &Value, key: &str) -> Result<String> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| anyhow!("challenge payload missing {key}"))
}

fn has_prepared_dns_payload(payload: &Value) -> bool {
    [
        "record_name",
        "record_value",
        "challenge_url",
        "authorization_url",
        "order_url",
        "finalize_url",
    ]
    .iter()
    .all(|key| payload.get(*key).and_then(Value::as_str).is_some())
}

fn pad_big_num(value: &openssl::bn::BigNumRef, width: usize) -> Vec<u8> {
    let bytes = value.to_vec();
    if bytes.len() >= width {
        return bytes;
    }
    let mut padded = vec![0; width - bytes.len()];
    padded.extend_from_slice(&bytes);
    padded
}

fn location_header(headers: &reqwest::header::HeaderMap) -> Result<String> {
    header_to_string(headers.get(LOCATION), "Location")
}

fn replay_nonce(headers: &reqwest::header::HeaderMap) -> Result<String> {
    header_to_string(headers.get("Replay-Nonce"), "Replay-Nonce")
}

fn header_to_string(value: Option<&HeaderValue>, name: &str) -> Result<String> {
    value
        .ok_or_else(|| anyhow!("missing {name} header"))?
        .to_str()
        .context("invalid header value")?
        .to_string()
        .pipe(Ok)
}

async fn ensure_success(success: bool, response: Response, action: &str) -> Result<Response> {
    if success {
        return Ok(response);
    }

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    bail!("{action} failed with status {status}: {body}");
}

async fn parse_json_response<T: DeserializeOwned>(response: Response, action: &str) -> Result<T> {
    let response = ensure_success(response.status().is_success(), response, action).await?;
    response
        .json::<T>()
        .await
        .with_context(|| format!("failed to decode {action} response json"))
}

async fn parse_text_response(response: Response, action: &str) -> Result<String> {
    let response = ensure_success(response.status().is_success(), response, action).await?;
    response
        .text()
        .await
        .with_context(|| format!("failed to decode {action} response text"))
}

fn generate_certificate_csr(
    common_name: &str,
    sans: &[String],
    key_passphrase: &[u8],
) -> Result<GeneratedCertificateRequest> {
    let ec_group = openssl::ec::EcGroup::from_curve_name(Nid::X9_62_PRIME256V1)
        .context("failed to create certificate ec group")?;
    let ec_key = openssl::ec::EcKey::generate(&ec_group)
        .context("failed to generate certificate private key")?;
    let pkey = PKey::from_ec_key(ec_key).context("failed to wrap certificate private key")?;
    let mut name_builder =
        openssl::x509::X509NameBuilder::new().context("failed to build certificate subject")?;
    name_builder
        .append_entry_by_nid(Nid::COMMONNAME, common_name)
        .context("failed to append certificate common name")?;
    let subject = name_builder.build();

    let mut req_builder =
        X509ReqBuilder::new().context("failed to construct certificate signing request")?;
    req_builder
        .set_subject_name(&subject)
        .context("failed to set csr subject")?;
    req_builder
        .set_pubkey(&pkey)
        .context("failed to set csr public key")?;

    let mut san_builder = SubjectAlternativeName::new();
    let unique_sans = unique_identifiers(common_name, sans);
    for dns_name in &unique_sans {
        san_builder.dns(dns_name);
    }
    let san_extension = san_builder
        .build(&req_builder.x509v3_context(None))
        .context("failed to build csr san extension")?;
    let mut extensions = Stack::new().context("failed to allocate csr extensions stack")?;
    extensions
        .push(san_extension)
        .context("failed to push csr san extension")?;
    req_builder
        .add_extensions(&extensions)
        .context("failed to add csr extensions")?;

    req_builder
        .sign(&pkey, MessageDigest::sha256())
        .context("failed to sign csr")?;
    let csr = req_builder.build();
    let csr_der = csr.to_der().context("failed to encode csr der")?;
    let key_pem_encrypted = String::from_utf8(
        pkey.private_key_to_pem_pkcs8_passphrase(Cipher::aes_256_cbc(), key_passphrase)
            .context("failed to encrypt certificate private key")?,
    )
    .context("failed to decode encrypted certificate private key pem")?;

    Ok(GeneratedCertificateRequest {
        csr_der,
        key_pem_encrypted,
    })
}

fn issued_material_from_chain(
    chain_pem: &str,
    key_pem_encrypted: String,
) -> Result<IssuedCertificateMaterial> {
    let certs = X509::stack_from_pem(chain_pem.as_bytes())
        .context("failed to parse downloaded certificate chain pem")?;
    let leaf = certs
        .first()
        .ok_or_else(|| anyhow!("empty certificate chain received from acme server"))?;
    let issuer = format_x509_name(leaf.issuer_name());
    let not_before = parse_asn1_time(leaf.not_before())?;
    let not_after = parse_asn1_time(leaf.not_after())?;
    let cert_pem = String::from_utf8(leaf.to_pem().context("failed to encode leaf cert pem")?)
        .context("failed to decode leaf cert pem")?;
    let chain_pem = if certs.len() > 1 {
        let mut chain = String::new();
        for cert in certs.iter().skip(1) {
            chain.push_str(
                &String::from_utf8(cert.to_pem().context("failed to encode chain cert pem")?)
                    .context("failed to decode chain cert pem")?,
            );
        }
        Some(chain)
    } else {
        None
    };
    let fingerprint_sha256 = leaf
        .digest(MessageDigest::sha256())
        .context("failed to calculate certificate fingerprint")?
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();

    Ok(IssuedCertificateMaterial {
        issuer,
        not_before,
        not_after,
        cert_pem,
        key_pem_encrypted,
        chain_pem,
        fingerprint_sha256,
    })
}

fn parse_asn1_time(value: &openssl::asn1::Asn1TimeRef) -> Result<DateTime<Utc>> {
    let raw = value.to_string();
    let parsed = NaiveDateTime::parse_from_str(&raw, "%b %e %H:%M:%S %Y GMT")
        .with_context(|| format!("failed to parse openssl time {raw}"))?;
    Ok(DateTime::<Utc>::from_naive_utc_and_offset(parsed, Utc))
}

fn format_x509_name(name: &X509NameRef) -> String {
    let common_name = name
        .entries_by_nid(Nid::COMMONNAME)
        .next()
        .and_then(|entry| entry.data().as_utf8().ok().map(|value| value.to_string()));
    if let Some(common_name) = common_name {
        return common_name;
    }

    let parts = name
        .entries()
        .filter_map(|entry| {
            let short_name = entry.object().nid().short_name().ok()?;
            let value = entry.data().as_utf8().ok()?;
            Some(format!("{short_name}={value}"))
        })
        .collect::<Vec<_>>();
    if parts.is_empty() {
        "unknown".to_string()
    } else {
        parts.join(", ")
    }
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}

impl<T> Pipe for T {}

#[derive(Debug, Serialize)]
struct EcJwk {
    crv: String,
    kty: String,
    x: String,
    y: String,
}

#[derive(Debug)]
struct GeneratedCertificateRequest {
    csr_der: Vec<u8>,
    key_pem_encrypted: String,
}

#[derive(Debug, Deserialize, Clone)]
struct AcmeDirectory {
    #[serde(rename = "newNonce")]
    new_nonce: String,
    #[serde(rename = "newAccount")]
    new_account: String,
    #[serde(rename = "newOrder")]
    new_order: String,
}

#[derive(Debug, Deserialize)]
struct AcmeOrder {
    status: String,
    authorizations: Vec<String>,
    finalize: String,
    certificate: Option<String>,
    error: Option<AcmeProblem>,
}

#[derive(Debug, Deserialize)]
struct AcmeAuthorization {
    status: String,
    identifier: AcmeIdentifier,
    challenges: Vec<AcmeAuthorizationChallenge>,
}

#[derive(Debug, Deserialize)]
struct AcmeIdentifier {
    value: String,
}

#[derive(Debug, Deserialize)]
struct AcmeAuthorizationChallenge {
    #[serde(rename = "type")]
    kind: String,
    url: String,
    token: Option<String>,
    error: Option<AcmeProblem>,
}

#[derive(Debug, Deserialize)]
struct AcmeProblem {
    detail: Option<String>,
}

enum JwsIdentity<'a> {
    Jwk,
    Kid(&'a str),
}

#[derive(Debug, Deserialize)]
struct CertificateChallengePayload {
    zone_name: Option<String>,
    record_name: Option<String>,
    record_type: Option<String>,
    record_value: Option<String>,
    ttl: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Default)]
    struct FakeStore {
        pending_jobs: Mutex<Vec<QueuedDnsChallengeOrder>>,
        issuance_jobs: Mutex<Vec<QueuedCertificateIssuanceOrder>>,
        statuses: Mutex<HashMap<Uuid, (String, Option<String>)>>,
        payloads: Mutex<HashMap<Uuid, Value>>,
        persisted: Mutex<Vec<PersistIssuedCertificateRequest>>,
    }

    #[async_trait]
    impl CertificateOrderStore for FakeStore {
        async fn claim_pending_dns_orders(
            &self,
            limit: usize,
        ) -> Result<Vec<QueuedDnsChallengeOrder>> {
            let mut jobs = self.pending_jobs.lock().await;
            let take = jobs.len().min(limit);
            Ok(jobs.drain(0..take).collect())
        }

        async fn update_order_challenge_payload(
            &self,
            order_id: Uuid,
            payload: &Value,
        ) -> Result<()> {
            self.payloads.lock().await.insert(order_id, payload.clone());
            Ok(())
        }

        async fn claim_ready_issuance_orders(
            &self,
            limit: usize,
        ) -> Result<Vec<QueuedCertificateIssuanceOrder>> {
            let mut jobs = self.issuance_jobs.lock().await;
            let take = jobs.len().min(limit);
            Ok(jobs.drain(0..take).collect())
        }

        async fn update_order_status(
            &self,
            order_id: Uuid,
            status: &str,
            error_message: Option<String>,
        ) -> Result<()> {
            self.statuses
                .lock()
                .await
                .insert(order_id, (status.to_string(), error_message));
            Ok(())
        }

        async fn persist_issued_certificate(
            &self,
            request: &PersistIssuedCertificateRequest,
        ) -> Result<()> {
            self.persisted.lock().await.push(request.clone());
            Ok(())
        }
    }

    fn pending_job(order_id: Uuid, payload: Value) -> QueuedDnsChallengeOrder {
        QueuedDnsChallengeOrder {
            order_id,
            site_id: Some(Uuid::new_v4()),
            certificate_id: Uuid::new_v4(),
            order_type: "issue".to_string(),
            acme_provider: "letsencrypt".to_string(),
            provider_type: "noop".to_string(),
            api_endpoint: None,
            credentials: "{}".to_string(),
            zone_name: "example.com".to_string(),
            external_zone_id: "noop-example.com".to_string(),
            challenge_payload: payload,
            common_name: "demo.example.com".to_string(),
            sans: vec!["demo.example.com".to_string()],
        }
    }

    fn issuance_job(order_id: Uuid, credentials: &str) -> QueuedCertificateIssuanceOrder {
        QueuedCertificateIssuanceOrder {
            order_id,
            site_id: Some(Uuid::new_v4()),
            certificate_id: Uuid::new_v4(),
            order_type: "issue".to_string(),
            acme_provider: "letsencrypt".to_string(),
            provider_type: "noop".to_string(),
            api_endpoint: None,
            credentials: credentials.to_string(),
            zone_name: "example.com".to_string(),
            external_zone_id: "noop-example.com".to_string(),
            challenge_payload: json!({
                "record_name": "_acme-challenge.demo.example.com",
                "record_type": "TXT",
                "record_value": "token-value",
                "ttl": 60,
                "challenge_url": "https://example.invalid/challenge",
                "authorization_url": "https://example.invalid/authz",
                "order_url": "https://example.invalid/order",
                "finalize_url": "https://example.invalid/finalize"
            }),
            common_name: "demo.example.com".to_string(),
            sans: vec!["demo.example.com".to_string()],
        }
    }

    #[tokio::test]
    async fn process_pending_orders_marks_success_for_noop_provider() {
        let order_id = Uuid::new_v4();
        let store = FakeStore {
            pending_jobs: Mutex::new(vec![pending_job(order_id, json!({}))]),
            ..Default::default()
        };

        let processed = process_pending_certificate_orders(&store, &MockAcmeClient, 10)
            .await
            .unwrap();

        assert_eq!(processed, 1);
        let statuses = store.statuses.lock().await;
        assert_eq!(
            statuses.get(&order_id),
            Some(&("dns_challenge_presented".to_string(), None))
        );
        let payloads = store.payloads.lock().await;
        assert!(
            payloads
                .get(&order_id)
                .and_then(|payload| payload.get("record_name"))
                .is_some()
        );
    }

    #[tokio::test]
    async fn process_pending_orders_marks_failure_for_invalid_payload() {
        let order_id = Uuid::new_v4();
        let store = FakeStore {
            pending_jobs: Mutex::new(vec![QueuedDnsChallengeOrder {
                credentials: "{".to_string(),
                ..pending_job(order_id, json!({}))
            }]),
            ..Default::default()
        };

        let processed = process_pending_certificate_orders(&store, &MockAcmeClient, 10)
            .await
            .unwrap();

        assert_eq!(processed, 1);
        let statuses = store.statuses.lock().await;
        let (status, error_message) = statuses.get(&order_id).unwrap();
        assert_eq!(status, "dns_challenge_failed");
        assert!(
            error_message
                .as_deref()
                .unwrap_or_default()
                .contains("invalid dns provider credentials")
        );
    }

    #[tokio::test]
    async fn process_presented_orders_persists_issued_certificate() {
        let order_id = Uuid::new_v4();
        let store = FakeStore {
            issuance_jobs: Mutex::new(vec![issuance_job(order_id, "{}")]),
            ..Default::default()
        };

        let processed = process_presented_certificate_orders(&store, &MockAcmeClient, 10)
            .await
            .unwrap();

        assert_eq!(processed, 1);
        let persisted = store.persisted.lock().await;
        assert_eq!(persisted.len(), 1);
        assert_eq!(persisted[0].order_id, order_id);
        assert_eq!(
            persisted[0].material.issuer,
            "PingoraHub Mock ACME (letsencrypt)"
        );
        assert!(persisted[0].material.cert_pem.contains("BEGIN CERTIFICATE"));
    }

    #[tokio::test]
    async fn process_presented_orders_marks_failure_for_bad_provider_credentials() {
        let order_id = Uuid::new_v4();
        let store = FakeStore {
            issuance_jobs: Mutex::new(vec![issuance_job(order_id, "{")]),
            ..Default::default()
        };

        let processed = process_presented_certificate_orders(&store, &MockAcmeClient, 10)
            .await
            .unwrap();

        assert_eq!(processed, 1);
        let statuses = store.statuses.lock().await;
        let (status, error_message) = statuses.get(&order_id).unwrap();
        assert_eq!(status, "issue_failed");
        assert!(
            error_message
                .as_deref()
                .unwrap_or_default()
                .contains("invalid dns provider credentials")
        );
    }

    #[test]
    fn active_renewal_status_guard_excludes_terminal_failures() {
        for status in [
            "pending_dns_challenge",
            "dns_challenge_presenting",
            "dns_challenge_presented",
            "issuing",
        ] {
            assert!(
                is_active_renewal_order_status(status),
                "{status} should block duplicate auto-renew orders"
            );
        }

        for status in ["issued", "dns_challenge_failed", "issue_failed", "canceled"] {
            assert!(
                !is_active_renewal_order_status(status),
                "{status} should not block future auto-renew attempts"
            );
        }
    }

    #[test]
    fn renewal_guard_migration_enforces_one_active_renewal_order() {
        let migration_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../migrations/0005_certificate_renewal_guard.sql");
        let migration = std::fs::read_to_string(&migration_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", migration_path.display()));

        assert!(
            migration.contains(
                "CREATE UNIQUE INDEX IF NOT EXISTS idx_certificate_orders_one_active_renew"
            )
        );
        assert!(migration.contains("certificate_id IS NOT NULL"));
        assert!(migration.contains("order_type = 'renew'"));
        assert!(migration.contains(
            "order_status IN ('pending_dns_challenge', 'dns_challenge_presenting', 'dns_challenge_presented', 'issuing')"
        ));
    }

    #[test]
    fn parse_asn1_time_accepts_openssl_gmt_format() {
        let time = openssl::asn1::Asn1Time::from_str_x509("20260713070053Z").unwrap();
        let parsed = parse_asn1_time(&time).unwrap();
        assert_eq!(parsed.to_rfc3339(), "2026-07-13T07:00:53+00:00");
    }

    #[test]
    fn dns01_record_name_strips_wildcard_label() {
        assert_eq!(
            dns01_record_name("*.example.com"),
            "_acme-challenge.example.com"
        );
        assert_eq!(
            dns01_record_name("api.example.com"),
            "_acme-challenge.api.example.com"
        );
    }
}
