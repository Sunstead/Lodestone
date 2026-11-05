//! Address names: a server with the name `survival` is reachable as
//! `survival.<public address>` (e.g. `survival.play.example.com`) through an
//! SRV record, `_minecraft._tcp.survival.play.example.com` →
//! `0 5 <game port> play.example.com`, so players type no port. Minecraft:
//! Java Edition looks that record up before anything else.
//!
//! The records are desired state, computed from Settings' public address and
//! every instance's name and port. A background task brings the DNS provider
//! in line whenever something changes, and every few minutes besides. It only
//! ever lists, changes or deletes records it created, which carry a comment
//! starting with [`MARKER`]; everything else in the zone is left alone.

use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::Notify;

use crate::app::Services;
use crate::config::DnsConfig;
use crate::error::{AppError, AppResult};
use crate::model::{DnsStatus, InstanceDns, ResourceKind};
use crate::store::InstanceRecord;

/// The comment on every record Lodestone manages, followed by the instance id.
pub const MARKER: &str = "lodestone:";
const PRIORITY: u16 = 0;
const WEIGHT: u16 = 5;
const TTL: u32 = 300;
const RESYNC: Duration = Duration::from_secs(10 * 60);
const RETRY: Duration = Duration::from_secs(60);

/// One SRV record as Lodestone wants it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SrvRecord {
    /// `_minecraft._tcp.<label>.<public address>`.
    pub name: String,
    /// The public address, which resolves to the edge.
    pub target: String,
    pub port: u16,
    pub instance_id: String,
}

impl SrvRecord {
    pub fn value(&self) -> String {
        format!("{PRIORITY} {WEIGHT} {} {}", self.port, self.target)
    }
}

/// Normalises and checks an address name: one DNS label, lowercase.
/// `None` for an empty name (no address name).
pub fn normalise_hostname(raw: &str) -> AppResult<Option<String>> {
    let name = raw.trim().to_ascii_lowercase();
    if name.is_empty() {
        return Ok(None);
    }
    let valid = name.len() <= 63
        && name.chars().all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
        && !name.starts_with('-')
        && !name.ends_with('-');
    if !valid {
        return Err(AppError::bad_request(
            "An address name is one word of letters, digits and hyphens (not at either end), up to 63 \
             characters, e.g. survival.",
        ));
    }
    Ok(Some(name))
}

/// Whether the public address can carry address names: a domain name, not
/// blank, an IP address or a host with a port.
pub fn is_domain(public_address: &str) -> bool {
    !public_address.is_empty()
        && public_address.parse::<IpAddr>().is_err()
        && !public_address.contains(':')
        && public_address.contains('.')
}

/// The record for one instance, if it has a name and the address allows it.
pub fn record_for(record: &InstanceRecord, public_address: &str) -> Option<SrvRecord> {
    let label = record.hostname.as_deref()?;
    is_domain(public_address).then(|| SrvRecord {
        name: format!("_minecraft._tcp.{label}.{public_address}"),
        target: public_address.to_owned(),
        port: record.ports.minecraft,
        instance_id: record.id.clone(),
    })
}

/// A record the provider holds, with the provider's id for it.
#[derive(Debug, Clone)]
pub struct Existing {
    pub id: String,
    pub record: SrvRecord,
}

#[async_trait]
pub trait DnsProvider: Send + Sync {
    /// The records carrying [`MARKER`], and no others.
    async fn list(&self) -> anyhow::Result<Vec<Existing>>;
    async fn create(&self, record: &SrvRecord) -> anyhow::Result<()>;
    async fn delete(&self, id: &str) -> anyhow::Result<()>;
}

#[derive(Default)]
struct SyncState {
    /// Records known to be in place after the last sync.
    present: HashSet<SrvRecord>,
    /// Why the last sync failed, if it did.
    error: Option<String>,
}

/// The DNS side of Lodestone: the provider, if any, and what the last sync found.
pub struct Dns {
    provider: Option<Arc<dyn DnsProvider>>,
    state: Mutex<SyncState>,
    /// Its own Arc, so the sync task can wait on it without keeping the app alive.
    wake: Arc<Notify>,
}

impl Dns {
    pub fn new(config: &DnsConfig, http: reqwest::Client) -> Self {
        let provider: Option<Arc<dyn DnsProvider>> = match config {
            DnsConfig::Manual => None,
            DnsConfig::Cloudflare { api_token, zone_id } => {
                Some(Arc::new(Cloudflare::new(http, CLOUDFLARE_API, api_token, zone_id)))
            }
            DnsConfig::Fake => Some(Arc::new(FakeDns::default())),
        };
        Self { provider, state: Mutex::default(), wake: Arc::default() }
    }

    /// Something that changes the records happened: sync soon.
    pub fn changed(&self) {
        self.wake.notify_one();
    }

    /// What the UI shows for one instance.
    pub fn status(&self, record: &InstanceRecord, public_address: &str) -> Option<InstanceDns> {
        let label = record.hostname.as_deref()?;
        let host = format!("{label}.{public_address}");
        let Some(srv) = record_for(record, public_address) else {
            return Some(InstanceDns {
                host,
                record: String::new(),
                value: String::new(),
                status: DnsStatus::Unavailable,
                message: Some(
                    "Address names need the public address in Settings to be a domain name, e.g. \
                     play.example.com."
                        .into(),
                ),
            });
        };
        let state = self.state.lock().unwrap();
        let (status, message) = if self.provider.is_none() {
            (
                DnsStatus::Manual,
                Some(format!("Add this SRV record for {public_address} at your DNS provider (DNS only, TTL {TTL}s).")),
            )
        } else if state.present.contains(&srv) {
            (DnsStatus::Synced, None)
        } else if let Some(error) = &state.error {
            (DnsStatus::Error, Some(format!("Couldn't update DNS: {error}. Lodestone keeps trying.")))
        } else {
            (DnsStatus::Pending, None)
        };
        Some(InstanceDns { host, record: srv.name.clone(), value: srv.value(), status, message })
    }

    /// Brings the provider in line with what the instances want. Returns how
    /// many records it created and deleted.
    pub async fn sync(&self, wanted: HashSet<SrvRecord>) -> anyhow::Result<(usize, usize)> {
        let Some(provider) = &self.provider else { return Ok((0, 0)) };
        let result = async {
            let existing = provider.list().await?;
            let mut present = HashSet::new();
            let mut deleted = 0;
            for found in existing {
                // A duplicate, or one no instance wants any more.
                if wanted.contains(&found.record) && present.insert(found.record.clone()) {
                    continue;
                }
                provider.delete(&found.id).await?;
                deleted += 1;
            }
            let mut created = 0;
            for record in wanted.difference(&present.clone()) {
                provider.create(record).await?;
                present.insert(record.clone());
                created += 1;
            }
            anyhow::Ok((present, created, deleted))
        }
        .await;
        let mut state = self.state.lock().unwrap();
        match result {
            Ok((present, created, deleted)) => {
                *state = SyncState { present, error: None };
                Ok((created, deleted))
            }
            Err(err) => {
                state.error = Some(err.to_string());
                Err(err)
            }
        }
    }
}

/// Every record the instances want right now.
pub async fn wanted(app: &Services) -> AppResult<HashSet<SrvRecord>> {
    let public_address = app.store.settings().await?.public_address;
    Ok(app.store.list_instances(None).await?.iter().filter_map(|record| record_for(record, &public_address)).collect())
}

/// Syncs on every change and every few minutes, for as long as the app lives.
pub fn spawn(app: Weak<Services>) {
    tokio::spawn(async move {
        loop {
            let Some(services) = app.upgrade() else { return };
            if services.dns.provider.is_none() {
                return;
            }
            let outcome = match wanted(&services).await {
                Ok(wanted) => services.dns.sync(wanted).await.map_err(|err| err.to_string()),
                Err(err) => Err(err.to_string()),
            };
            let wait = match outcome {
                Ok((created, deleted)) => {
                    if created + deleted > 0 {
                        tracing::info!(created, deleted, "DNS records updated");
                    }
                    RESYNC
                }
                Err(err) => {
                    tracing::warn!(%err, "couldn't update DNS records");
                    RETRY
                }
            };
            services.events.changed(ResourceKind::Instances, None);
            let wake = services.dns.wake.clone();
            drop(services);
            tokio::select! {
                () = wake.notified() => {}
                () = tokio::time::sleep(wait) => {}
            }
        }
    });
}

/* ------------------------------------------------------------------------ */
/* Cloudflare                                                               */
/* ------------------------------------------------------------------------ */

pub const CLOUDFLARE_API: &str = "https://api.cloudflare.com/client/v4";

/// Cloudflare's DNS API, with a token allowed Zone → DNS → Edit on one zone.
pub struct Cloudflare {
    http: reqwest::Client,
    records_url: String,
    token: String,
}

#[derive(Deserialize)]
struct CfResponse<T> {
    success: bool,
    #[serde(default)]
    errors: Vec<CfError>,
    result: Option<T>,
    result_info: Option<CfPage>,
}

#[derive(Deserialize)]
struct CfError {
    message: String,
}

#[derive(Deserialize)]
struct CfPage {
    total_pages: Option<u32>,
}

#[derive(Deserialize)]
struct CfRecord {
    id: String,
    name: String,
    comment: Option<String>,
    data: Option<CfSrvData>,
}

#[derive(Deserialize)]
struct CfSrvData {
    port: u16,
    target: String,
}

impl Cloudflare {
    pub fn new(http: reqwest::Client, api: &str, token: &str, zone_id: &str) -> Self {
        Self {
            http,
            records_url: format!("{}/zones/{zone_id}/dns_records", api.trim_end_matches('/')),
            token: token.to_owned(),
        }
    }

    async fn send<T: serde::de::DeserializeOwned>(
        &self,
        request: reqwest::RequestBuilder,
    ) -> anyhow::Result<(Option<T>, Option<CfPage>)> {
        let response = request.bearer_auth(&self.token).send().await?;
        let status = response.status();
        let body: CfResponse<T> = response
            .json()
            .await
            .map_err(|err| anyhow::anyhow!("Cloudflare answered {status} with something unexpected: {err}"))?;
        if !body.success || !status.is_success() {
            let messages: Vec<_> = body.errors.into_iter().map(|error| error.message).collect();
            anyhow::bail!("Cloudflare answered {status}: {}", messages.join("; "));
        }
        Ok((body.result, body.result_info))
    }
}

#[async_trait]
impl DnsProvider for Cloudflare {
    async fn list(&self) -> anyhow::Result<Vec<Existing>> {
        let mut out = Vec::new();
        let mut page = 1;
        loop {
            let request = self.http.get(&self.records_url).query(&[
                ("type", "SRV"),
                ("comment.startswith", MARKER),
                ("per_page", "100"),
                ("page", &page.to_string()),
            ]);
            let (records, info) = self.send::<Vec<CfRecord>>(request).await?;
            for record in records.unwrap_or_default() {
                // Belt and braces: never touch a record without the marker.
                let Some(instance_id) = record.comment.as_deref().and_then(|c| c.strip_prefix(MARKER)) else {
                    continue;
                };
                let Some(data) = record.data else { continue };
                out.push(Existing {
                    id: record.id,
                    record: SrvRecord {
                        name: record.name,
                        target: data.target.trim_end_matches('.').to_owned(),
                        port: data.port,
                        instance_id: instance_id.to_owned(),
                    },
                });
            }
            if page >= info.and_then(|info| info.total_pages).unwrap_or(1) {
                return Ok(out);
            }
            page += 1;
        }
    }

    async fn create(&self, record: &SrvRecord) -> anyhow::Result<()> {
        let body = json!({
            "type": "SRV",
            "name": record.name,
            "ttl": TTL,
            "data": { "priority": PRIORITY, "weight": WEIGHT, "port": record.port, "target": record.target },
            "comment": format!("{MARKER}{}", record.instance_id),
        });
        self.send::<serde_json::Value>(self.http.post(&self.records_url).json(&body)).await?;
        Ok(())
    }

    async fn delete(&self, id: &str) -> anyhow::Result<()> {
        self.send::<serde_json::Value>(self.http.delete(format!("{}/{id}", self.records_url))).await?;
        Ok(())
    }
}

/* ------------------------------------------------------------------------ */
/* In-memory stand-in                                                       */
/* ------------------------------------------------------------------------ */

/// Records kept in memory, for the fake runtime and tests.
#[derive(Default)]
pub struct FakeDns {
    records: Mutex<HashMap<String, SrvRecord>>,
}

#[async_trait]
impl DnsProvider for FakeDns {
    async fn list(&self) -> anyhow::Result<Vec<Existing>> {
        let records = self.records.lock().unwrap();
        Ok(records.iter().map(|(id, record)| Existing { id: id.clone(), record: record.clone() }).collect())
    }

    async fn create(&self, record: &SrvRecord) -> anyhow::Result<()> {
        self.records.lock().unwrap().insert(crate::util::new_id("dns"), record.clone());
        Ok(())
    }

    async fn delete(&self, id: &str) -> anyhow::Result<()> {
        self.records.lock().unwrap().remove(id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::InstancePorts;
    use wiremock::matchers::{body_partial_json, header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn instance(id: &str, hostname: Option<&str>, port: u16) -> InstanceRecord {
        InstanceRecord {
            id: id.into(),
            name: id.into(),
            profile_id: "p".into(),
            world_id: format!("w-{id}"),
            ports: InstancePorts { minecraft: port, rcon: 40000, query: None },
            hostname: hostname.map(Into::into),
            environment: Default::default(),
            rcon_password: String::new(),
            created_at: String::new(),
            last_error: None,
        }
    }

    #[test]
    fn hostnames_are_one_label() {
        assert_eq!(normalise_hostname(" Survival ").unwrap(), Some("survival".into()));
        assert_eq!(normalise_hostname("sky-block2").unwrap(), Some("sky-block2".into()));
        assert_eq!(normalise_hostname("").unwrap(), None);
        for bad in ["a.b", "-x", "x-", "café", "two words", &"x".repeat(64)] {
            assert!(normalise_hostname(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn records_need_a_domain() {
        let record = record_for(&instance("i1", Some("survival"), 30001), "play.example.com").unwrap();
        assert_eq!(record.name, "_minecraft._tcp.survival.play.example.com");
        assert_eq!(record.value(), "0 5 30001 play.example.com");
        for address in ["", "203.0.113.7", "play.example.com:30000", "localhost"] {
            assert!(record_for(&instance("i1", Some("survival"), 30001), address).is_none(), "{address}");
        }
        assert!(record_for(&instance("i1", None, 30001), "play.example.com").is_none());
    }

    #[tokio::test]
    async fn sync_creates_moves_and_removes_only_its_own() {
        let dns = Dns::new(&DnsConfig::Fake, reqwest::Client::new());
        let a = record_for(&instance("a", Some("survival"), 30001), "play.example.com").unwrap();
        let b = record_for(&instance("b", Some("creative"), 30002), "play.example.com").unwrap();
        assert_eq!(dns.sync(HashSet::from([a.clone(), b.clone()])).await.unwrap(), (2, 0));
        assert_eq!(
            dns.status(&instance("a", Some("survival"), 30001), "play.example.com").unwrap().status,
            DnsStatus::Synced
        );
        // Already right: nothing to do.
        assert_eq!(dns.sync(HashSet::from([a.clone(), b.clone()])).await.unwrap(), (0, 0));
        // b moved port, a was removed.
        let moved = record_for(&instance("b", Some("creative"), 30003), "play.example.com").unwrap();
        assert_eq!(dns.sync(HashSet::from([moved.clone()])).await.unwrap(), (1, 2));
        assert_eq!(
            dns.status(&instance("b", Some("creative"), 30002), "play.example.com").unwrap().status,
            DnsStatus::Pending
        );
    }

    #[test]
    fn manual_and_unavailable_say_what_to_do() {
        let dns = Dns::new(&DnsConfig::Manual, reqwest::Client::new());
        let status = dns.status(&instance("a", Some("survival"), 30001), "play.example.com").unwrap();
        assert_eq!(status.status, DnsStatus::Manual);
        assert_eq!(status.record, "_minecraft._tcp.survival.play.example.com");
        assert!(status.message.unwrap().contains("SRV"));
        let status = dns.status(&instance("a", Some("survival"), 30001), "203.0.113.7").unwrap();
        assert_eq!(status.status, DnsStatus::Unavailable);
        assert!(dns.status(&instance("a", None, 30001), "play.example.com").is_none());
    }

    #[tokio::test]
    async fn cloudflare_lists_creates_and_deletes_marked_records() {
        let server = MockServer::start().await;
        let base = "/zones/zone1/dns_records";
        Mock::given(method("GET"))
            .and(path(base))
            .and(query_param("type", "SRV"))
            .and(query_param("comment.startswith", MARKER))
            .and(header("authorization", "Bearer tok"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "success": true, "errors": [],
                "result": [
                    { "id": "keep", "name": "_minecraft._tcp.survival.play.example.com", "comment": "lodestone:a",
                      "data": { "priority": 0, "weight": 5, "port": 30001, "target": "play.example.com" } },
                    { "id": "stale", "name": "_minecraft._tcp.old.play.example.com", "comment": "lodestone:gone",
                      "data": { "priority": 0, "weight": 5, "port": 30009, "target": "play.example.com" } },
                    { "id": "theirs", "name": "_minecraft._tcp.x.example.com", "comment": "someone else's",
                      "data": { "priority": 0, "weight": 5, "port": 1, "target": "example.com" } }
                ],
                "result_info": { "page": 1, "total_pages": 1 }
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path(format!("{base}/stale")))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "success": true, "result": { "id": "stale" } })),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path(base))
            .and(body_partial_json(json!({
                "type": "SRV", "name": "_minecraft._tcp.creative.play.example.com", "comment": "lodestone:b",
                "data": { "port": 30002, "target": "play.example.com", "priority": 0, "weight": 5 }
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "success": true, "result": {} })))
            .expect(1)
            .mount(&server)
            .await;

        let provider = Cloudflare::new(reqwest::Client::new(), &server.uri(), "tok", "zone1");
        let dns = Dns { provider: Some(Arc::new(provider)), state: Mutex::default(), wake: Arc::default() };
        let a = record_for(&instance("a", Some("survival"), 30001), "play.example.com").unwrap();
        let b = record_for(&instance("b", Some("creative"), 30002), "play.example.com").unwrap();
        assert_eq!(dns.sync(HashSet::from([a, b])).await.unwrap(), (1, 1));
    }

    #[tokio::test]
    async fn cloudflare_errors_are_reported() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(403).set_body_json(json!({
                "success": false, "errors": [{ "code": 10000, "message": "Authentication error" }], "result": null
            })))
            .mount(&server)
            .await;
        let provider = Cloudflare::new(reqwest::Client::new(), &server.uri(), "bad", "zone1");
        let dns = Dns { provider: Some(Arc::new(provider)), state: Mutex::default(), wake: Arc::default() };
        let a = record_for(&instance("a", Some("survival"), 30001), "play.example.com").unwrap();
        let err = dns.sync(HashSet::from([a])).await.unwrap_err().to_string();
        assert!(err.contains("Authentication error"), "{err}");
        let status = dns.status(&instance("a", Some("survival"), 30001), "play.example.com").unwrap();
        assert_eq!(status.status, DnsStatus::Error);
        assert!(status.message.unwrap().contains("Authentication error"));
    }
}
