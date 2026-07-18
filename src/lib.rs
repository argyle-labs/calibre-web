//! calibre-web service backend — Ebook web library/reader.
//!
//! Implements `ServiceBackend` so the generic `service.*` tools
//! (deploy/backup/restore/configure/status/connect/sync) drive calibre-web. No
//! `#[orca_tool]`s — the only orca dep is `plugin-toolkit`. Modeled on the
//! nfs StorageBackend. See orca/docs/PLUGIN-PROGRAM.md.
#![allow(clippy::disallowed_types)]

use plugin_toolkit::deploy_target::{EnvVar, Mount};
use plugin_toolkit::service::{
    BoxFuture, Endpoint, Runtime, ServiceBackend, ServiceCapability, ServiceError, ServiceStatus,
    WorkloadSpec,
};

/// Upstream image. We run **Calibre-Web-Automated** (CWA) rather than plain
/// calibre-web: CWA adds an auto-ingest folder (`/cwa-book-ingest`) that
/// converts + imports dropped files and runs installed Calibre plugins
/// (DeDRM/Obok) on the way in — the ingest capability orca wants to drive.
const CWA_IMAGE: &str = "crocodilestick/calibre-web-automated";
/// Host dir holding CWA config + the persisted Calibre plugin set
/// (`<config>/.config/calibre/plugins/` — where DeDRM must live to survive
/// container recreation).
const DEFAULT_CONFIG_PATH: &str = "/opt/appdata/calibre-web";
/// Base of the fleet media tree (NFS from the storage tier). The Calibre
/// library lives at `<media>/books`; new files land in `<media>/book-ingest`.
const DEFAULT_MEDIA_PATH: &str = "/mnt/pool/data/media";

/// calibre-web backend. Holds only the provider name; per-instance endpoint/creds
/// come from the `Endpoint` the generic `service.*` tools hand each op.
#[derive(Debug, Clone)]
pub struct CalibreWebBackend {
    provider: &'static str,
}

impl CalibreWebBackend {
    pub fn new(provider: &'static str) -> Self {
        Self { provider }
    }
}

impl ServiceBackend for CalibreWebBackend {
    fn provider(&self) -> &str {
        self.provider
    }

    /// Runtimes calibre-web can be placed on. `service.deploy` hands the
    /// `workload_spec` below to a matching deploy target — this backend never
    /// drives pct/docker itself (that mechanic lives in the deploy-target domain).
    fn runtimes(&self) -> Vec<Runtime> {
        vec![Runtime::Docker, Runtime::Podman, Runtime::Lxc, Runtime::Vm]
    }

    fn capabilities(&self) -> Vec<ServiceCapability> {
        vec![
            ServiceCapability::Deploy,
            ServiceCapability::Backup,
            ServiceCapability::Restore,
            ServiceCapability::Configure,
            ServiceCapability::Status,
        ]
    }

    fn default_port(&self) -> u16 {
        8083
    }

    /// In-workload paths holding config/data. This is ALL calibre-web declares for
    /// backup — the generic pluggable backup (tar for containers/LXC, PBS for
    /// Proxmox guests when available) snapshots these. No backup/restore code
    /// here; those are inherited from ServiceBackend's defaults.
    fn data_paths(&self) -> Vec<String> {
        vec!["/config".to_string()]
    }

    /// Describe the CWA workload for the chosen runtime. Runtime-agnostic on
    /// purpose: `service.deploy` hands this to whichever deploy target owns the
    /// requested host (a Docker engine on baldur, an LXC, …) and that adapter
    /// binds it to its native form. The plugin never drives docker/pct itself.
    ///
    /// Paths default to the fleet layout; the library + ingest dirs sit under
    /// the same NFS media base so any host that mounts it sees one library.
    fn workload_spec<'a>(
        &'a self,
        _runtime: Runtime,
        ep: &'a Endpoint,
    ) -> BoxFuture<'a, Result<WorkloadSpec, ServiceError>> {
        let name = if ep.name.is_empty() {
            self.provider.to_string()
        } else {
            ep.name.clone()
        };
        let port = self.default_port();
        Box::pin(async move {
            Ok(WorkloadSpec {
                name,
                image: Some(format!("{CWA_IMAGE}:latest")),
                env: vec![
                    EnvVar {
                        key: "TZ".into(),
                        value: "America/Denver".into(),
                    },
                    EnvVar {
                        key: "PUID".into(),
                        value: "1000".into(),
                    },
                    EnvVar {
                        key: "PGID".into(),
                        value: "1000".into(),
                    },
                ],
                mounts: vec![
                    // Config + persisted Calibre plugins (DeDRM/Obok).
                    Mount {
                        source: DEFAULT_CONFIG_PATH.into(),
                        target: "/config".into(),
                        read_only: false,
                    },
                    // Existing Calibre library (holds metadata.db).
                    Mount {
                        source: format!("{DEFAULT_MEDIA_PATH}/books"),
                        target: "/calibre-library".into(),
                        read_only: false,
                    },
                    // Auto-ingest drop folder — files here are converted,
                    // de-DRM'd, and imported automatically by CWA.
                    Mount {
                        source: format!("{DEFAULT_MEDIA_PATH}/book-ingest"),
                        target: "/cwa-book-ingest".into(),
                        read_only: false,
                    },
                ],
                ports: vec![format!("{port}:{port}")],
            })
        })
    }

    fn configure<'a>(
        &'a self,
        _ep: &'a Endpoint,
        _config: &'a str,
    ) -> BoxFuture<'a, Result<(), ServiceError>> {
        // TODO: apply calibre-web-specific config idempotently.
        Box::pin(async move { Err(ServiceError::unimplemented("calibre-web.configure")) })
    }

    fn status<'a>(
        &'a self,
        _ep: &'a Endpoint,
    ) -> BoxFuture<'a, Result<ServiceStatus, ServiceError>> {
        // TODO: real health/diagnostics.
        Box::pin(async move { Err(ServiceError::unimplemented("calibre-web.status")) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declares_provider() {
        let b = CalibreWebBackend::new("calibre-web");
        assert_eq!(b.provider(), "calibre-web");
    }

    #[tokio::test]
    async fn workload_spec_describes_cwa() {
        let b = CalibreWebBackend::new("calibre-web");
        let ep = Endpoint {
            name: String::new(),
            base_url: String::new(),
            target_host: "baldur".into(),
            runtime: Some(Runtime::Docker),
            backup_method: None,
            token: String::new(),
        };
        let spec = b.workload_spec(Runtime::Docker, &ep).await.unwrap();

        // Falls back to the provider name when the endpoint is unnamed.
        assert_eq!(spec.name, "calibre-web");
        // Runs CWA (auto-ingest + de-DRM), not plain calibre-web.
        assert_eq!(
            spec.image.as_deref(),
            Some("crocodilestick/calibre-web-automated:latest")
        );
        assert!(spec.ports.contains(&"8083:8083".to_string()));
        // The three volumes CWA needs: config, library, and the ingest drop.
        let targets: Vec<&str> = spec.mounts.iter().map(|m| m.target.as_str()).collect();
        assert!(targets.contains(&"/config"));
        assert!(targets.contains(&"/calibre-library"));
        assert!(targets.contains(&"/cwa-book-ingest"));
    }

    #[tokio::test]
    async fn workload_spec_uses_endpoint_name_when_set() {
        let b = CalibreWebBackend::new("calibre-web");
        let ep = Endpoint {
            name: "calibre-web-main".into(),
            base_url: String::new(),
            target_host: "baldur".into(),
            runtime: Some(Runtime::Docker),
            backup_method: None,
            token: String::new(),
        };
        let spec = b.workload_spec(Runtime::Docker, &ep).await.unwrap();
        assert_eq!(spec.name, "calibre-web-main");
    }
}
