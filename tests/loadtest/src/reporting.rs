use crate::error::Result;
use crate::{deserialize_url, serialize_url};
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetEnvironment {
    #[serde(serialize_with = "serialize_url")]
    #[serde(deserialize_with = "deserialize_url")]
    pub endpoint: Url,
    pub deployment_mode: DeploymentMode,
    pub resources: Option<ResourceSpec>,
    pub storage: Option<StorageInfo>,
    pub build_info: BuildInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DeploymentMode {
    Local {
        #[serde(skip_serializing_if = "Option::is_none")]
        pid: Option<u32>,
    },
    Remote {
        server_type: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        location: Option<String>,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceSpec {
    pub vcpu: usize,
    pub memory_gb: usize,
    pub os: String,
    pub os_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageInfo {
    pub storage_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildInfo {
    pub rust_version: String,
    pub save_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_commit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
}

impl TargetEnvironment {
    pub fn detect(endpoint: Url) -> Result<Self> {
        let (deployment_mode, resources) = if let (Some(server_type), Some(vcpu), Some(memory_gb)) = (
            std::env::var("SAVE_SERVER_TYPE").ok(),
            std::env::var("SAVE_SERVER_VCPU")
                .ok()
                .and_then(|s| s.parse().ok()),
            std::env::var("SAVE_SERVER_RAM_GB")
                .ok()
                .and_then(|s| s.parse().ok()),
        ) {
            let os = std::env::var("SAVE_SERVER_OS").unwrap_or_else(|_| "Linux".to_string());
            let os_version = std::env::var("SAVE_SERVER_OS_VERSION")
                .unwrap_or_else(|_| "Ubuntu 24.04".to_string());

            let location = std::env::var("SAVE_SERVER_LOCATION").ok();

            (
                DeploymentMode::Remote {
                    server_type,
                    location,
                },
                Some(ResourceSpec {
                    vcpu,
                    memory_gb,
                    os,
                    os_version,
                }),
            )
        } else {
            let pid = std::env::var("SERVER_PID")
                .ok()
                .and_then(|s| s.parse().ok());

            let resources = Some(ResourceSpec {
                vcpu: num_cpus::get(),
                memory_gb: (sys_info::mem_info()
                    .map(|m| m.total / 1024 / 1024)
                    .unwrap_or(0)) as usize,
                os: std::env::consts::OS.to_string(),
                os_version: detect_os_version(),
            });

            (DeploymentMode::Local { pid }, resources)
        };

        let build_info = BuildInfo::detect();

        let storage = std::env::var("SAVE_STORAGE_TYPE")
            .ok()
            .filter(|s| !s.is_empty())
            .map(|storage_type| StorageInfo { storage_type });

        Ok(Self {
            endpoint,
            deployment_mode,
            resources,
            storage,
            build_info,
        })
    }
}

impl BuildInfo {
    pub fn detect() -> Self {
        let git_commit = std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|s| s.trim().to_string());

        let git_branch = std::process::Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|s| s.trim().to_string());

        Self {
            rust_version: format!("rustc {}", rustc_version_runtime::version()),
            save_version: env!("CARGO_PKG_VERSION").to_string(),
            git_commit,
            git_branch,
        }
    }
}

fn detect_os_version() -> String {
    #[cfg(target_os = "linux")]
    {
        sys_info::linux_os_release()
            .ok()
            .map(|info| {
                format!(
                    "{} {}",
                    info.name.unwrap_or_default(),
                    info.version.unwrap_or_default()
                )
            })
            .unwrap_or_else(|| "Linux".to_string())
    }

    #[cfg(not(target_os = "linux"))]
    {
        format!("{}", std::env::consts::OS)
    }
}
