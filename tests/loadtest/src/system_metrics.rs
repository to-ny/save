use serde::{Deserialize, Serialize};
use sysinfo::{
    CpuRefreshKind, MemoryRefreshKind, Pid, ProcessRefreshKind, ProcessesToUpdate, RefreshKind,
    System,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemMetrics {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub cpu_percent: f64,
    pub memory_mb: f64,
    pub disk_read_mb: f64,
    pub disk_write_mb: f64,
}

pub struct SystemCollector {
    system: System,
    pid: Pid,
    last_disk_read: u64,
    last_disk_write: u64,
}

impl SystemCollector {
    pub fn new(pid: u32) -> Self {
        let refresh_kind = RefreshKind::nothing()
            .with_cpu(CpuRefreshKind::everything())
            .with_memory(MemoryRefreshKind::everything())
            .with_processes(ProcessRefreshKind::everything());

        let mut system = System::new_with_specifics(refresh_kind);
        system.refresh_all();

        Self {
            system,
            pid: Pid::from_u32(pid),
            last_disk_read: 0,
            last_disk_write: 0,
        }
    }

    pub fn collect(&mut self) -> Option<SystemMetrics> {
        self.system
            .refresh_processes(ProcessesToUpdate::Some(&[self.pid]), true);

        let process = self.system.process(self.pid)?;

        let cpu_percent = process.cpu_usage() as f64;
        let memory_mb = process.memory() as f64 / 1_048_576.0;

        let disk_usage = process.disk_usage();
        let current_read = disk_usage.total_read_bytes;
        let current_write = disk_usage.total_written_bytes;

        let disk_read_mb = (current_read - self.last_disk_read) as f64 / 1_048_576.0;
        let disk_write_mb = (current_write - self.last_disk_write) as f64 / 1_048_576.0;

        self.last_disk_read = current_read;
        self.last_disk_write = current_write;

        Some(SystemMetrics {
            timestamp: chrono::Utc::now(),
            cpu_percent,
            memory_mb,
            disk_read_mb,
            disk_write_mb,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collector_creates() {
        let pid = std::process::id();
        let mut collector = SystemCollector::new(pid);
        let metrics = collector.collect();
        assert!(metrics.is_some());
        let m = metrics.unwrap();
        assert!(m.memory_mb > 0.0);
    }
}
