// Service registry + manifest builder.
//
// `services.toml` is baked into the binary at compile time (include_str!) so the
// image ships with the registry and needs no runtime file. The manifest is
// computed live per request: `enabled` for type=web reflects whether the
// service's container is currently TCP-reachable (which mirrors compose
// profiles), and `enabled` for type=agent reflects an env flag (default true).

use serde::{Deserialize, Serialize};
use std::time::Duration;

const SERVICES_TOML: &str = include_str!("../services.toml");

/// Wrapper for the `[[service]]` array in `services.toml` (TOML's array-of-
/// tables deserializes to a map with one key, not a bare sequence).
#[derive(Debug, Deserialize)]
struct ServicesFile {
    service: Vec<Service>,
}

/// A service declared in `services.toml`.
#[derive(Debug, Clone, Deserialize)]
pub struct Service {
    pub id: String,
    #[serde(rename = "type")]
    pub service_type: ServiceType,
    /// `host:port` to TCP-probe for liveness (type=web only).
    pub target: Option<String>,
    /// Gateway path the iframe opens (type=web only).
    pub url: Option<String>,
    /// Env-var name gating the pane (type=agent only).
    pub enable: Option<String>,
    /// Command launched in the pty; "" = default shell (type=agent only).
    pub cmd: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ServiceType {
    Web,
    Agent,
}

/// One entry in the JSON manifest returned by `GET /api/manifest`.
///
/// `url` is present only for `type=web`; `cmd` only for `type=agent` (the
/// inapplicable field is omitted, not null - see design §4).
#[derive(Debug, Serialize)]
pub struct ManifestEntry {
    pub id: String,
    #[serde(rename = "type")]
    pub service_type: ServiceType,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cmd: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Manifest {
    pub services: Vec<ManifestEntry>,
}

/// Parse the baked-in `services.toml` once at startup.
///
/// Panics at startup if the source file is invalid - this is a compile-time
/// asset, so a parse error is a programmer bug, not a runtime condition.
pub fn load_services() -> Vec<Service> {
    toml::from_str::<ServicesFile>(SERVICES_TOML)
        .expect("services.toml is invalid - fix the source file")
        .service
}

/// Build the live manifest: compute `enabled` for every service.
pub async fn build_manifest(services: &[Service]) -> Manifest {
    let mut entries = Vec::with_capacity(services.len());
    for svc in services {
        let enabled = match svc.service_type {
            ServiceType::Web => is_web_reachable(svc).await,
            ServiceType::Agent => is_agent_enabled(svc),
        };
        let (url, cmd) = match svc.service_type {
            ServiceType::Web => (svc.url.clone(), None),
            ServiceType::Agent => (None, svc.cmd.clone()),
        };
        entries.push(ManifestEntry {
            id: svc.id.clone(),
            service_type: svc.service_type,
            enabled,
            url,
            cmd,
        });
    }
    Manifest { services: entries }
}

/// type=web liveness: TCP-connect to `target` with a short timeout. A down or
/// absent compose service fails fast (NXDOMAIN or refused) without hanging the
/// manifest. Any error (DNS / connect / timeout) => not enabled.
async fn is_web_reachable(svc: &Service) -> bool {
    let Some(target) = &svc.target else {
        return false;
    };
    let connect = tokio::net::TcpStream::connect(target);
    match tokio::time::timeout(Duration::from_millis(400), connect).await {
        Ok(Ok(_)) => true,
        _ => false,
    }
}

/// type=agent liveness: read the env var named by `enable`. Absent or any value
/// other than "false"/"0" => enabled (default true).
fn is_agent_enabled(svc: &Service) -> bool {
    let Some(var) = &svc.enable else {
        return true;
    };
    match std::env::var(var) {
        Ok(v) => !matches!(v.as_str(), "false" | "0"),
        Err(_) => true,
    }
}
