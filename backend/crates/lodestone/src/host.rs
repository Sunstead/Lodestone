//! Host metrics, sampled in the background so requests never block on them.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use sysinfo::{CpuRefreshKind, Disks, MemoryRefreshKind, RefreshKind, System};

use crate::model::HostUsage;

#[derive(Clone)]
pub struct HostSampler {
    latest: Arc<RwLock<HostUsage>>,
}

impl HostSampler {
    /// Starts sampling every two seconds. `data_dir` picks which disk to report.
    pub fn start(data_dir: PathBuf) -> Self {
        let latest = Arc::new(RwLock::new(HostUsage::default()));
        let sampler = Self { latest: latest.clone() };
        tokio::task::spawn_blocking(move || {
            let mut system = System::new_with_specifics(
                RefreshKind::nothing()
                    .with_cpu(CpuRefreshKind::nothing().with_cpu_usage())
                    .with_memory(MemoryRefreshKind::everything()),
            );
            let mut disks = Disks::new_with_refreshed_list();
            loop {
                system.refresh_cpu_usage();
                system.refresh_memory();
                disks.refresh(true);
                let (disk_used, disk_total) = disk_for(&disks, &data_dir);
                let usage = HostUsage {
                    cpu_percent: system.global_cpu_usage() as f64,
                    cpu_cores: system.cpus().len() as u32,
                    disk_used_bytes: disk_used,
                    disk_total_bytes: disk_total,
                    swap_used_bytes: system.used_swap(),
                    swap_total_bytes: system.total_swap(),
                };
                if let Ok(mut slot) = latest.write() {
                    *slot = usage;
                }
                if Arc::strong_count(&latest) == 1 {
                    return; // every sampler handle is gone
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        });
        sampler
    }

    pub fn latest(&self) -> HostUsage {
        self.latest.read().map(|usage| *usage).unwrap_or_default()
    }
}

/// `(used, total)` for the disk holding `path` (longest matching mount point).
fn disk_for(disks: &Disks, path: &Path) -> (u64, u64) {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    disks
        .list()
        .iter()
        .filter(|disk| path.starts_with(disk.mount_point()))
        .max_by_key(|disk| disk.mount_point().as_os_str().len())
        .map(|disk| (disk.total_space() - disk.available_space(), disk.total_space()))
        .unwrap_or((0, 0))
}
