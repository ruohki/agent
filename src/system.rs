use serde::Serialize;
use sysinfo::{Disks, System};
use std::collections::HashMap;
use anyhow::Result;

#[derive(Serialize, Debug)]
pub struct SystemInfo {
    pub os: String,
    pub arch: String,
    pub platform: String,
    pub kernel: String,
    pub distribution: String,
    pub version: String,
}

#[derive(Serialize, Debug)]
pub struct DiskUsage {
    pub total: u64,
    pub used: u64,
    pub available: u64,
}

#[derive(Serialize, Debug)]
pub struct MemoryUsage {
    pub total: u64,
    pub used: u64,
    pub available: u64,
}

#[cfg(target_os = "linux")]
fn get_linux_distribution() -> Option<String> {
    use std::fs;
    
    // Try /etc/os-release first
    if let Ok(content) = fs::read_to_string("/etc/os-release") {
        for line in content.lines() {
            if line.starts_with("NAME=") {
                return Some(line[5..].trim_matches('"').to_string());
            }
        }
    }

    // Fallback to /etc/issue
    if let Ok(content) = fs::read_to_string("/etc/issue") {
        return Some(content.lines().next()?.trim().to_string());
    }

    None
}

pub fn collect_system_info() -> Result<SystemInfo> {
    let os_name = System::name().unwrap_or_else(|| "Unknown".to_string());
    let arch = System::cpu_arch().unwrap_or_else(|| "Unknown".to_string());
    let kernel_version = System::kernel_version().unwrap_or_else(|| "Unknown".to_string());
    let os_version = System::os_version().unwrap_or_else(|| "Unknown".to_string());

    // Determine platform based on OS
    let platform = if cfg!(target_os = "linux") {
        "linux".to_string()
    } else if cfg!(target_os = "macos") {
        "darwin".to_string()
    } else if cfg!(target_os = "windows") {
        "windows".to_string()
    } else {
        "unknown".to_string()
    };

    // Try to get distribution info on Linux
    let distribution = {
        #[cfg(target_os = "linux")]
        {
            get_linux_distribution().unwrap_or_else(|| os_name.clone())
        }
        #[cfg(not(target_os = "linux"))]
        {
            os_name.clone()
        }
    };

    Ok(SystemInfo {
        os: os_name,
        arch,
        platform,
        kernel: kernel_version,
        distribution,
        version: os_version,
    })
}

pub fn collect_hostname() -> Result<String> {
    hostname::get()
        .map_err(|e| anyhow::anyhow!("Failed to get hostname: {}", e))?
        .to_string_lossy()
        .to_string()
        .pipe(Ok)
}

pub fn collect_load_average() -> Option<[f64; 3]> {
    #[cfg(unix)]
    {
        let load = System::load_average();
        Some([load.one, load.five, load.fifteen])
    }
    #[cfg(not(unix))]
    {
        None
    }
}

pub fn collect_disk_usage() -> HashMap<String, DiskUsage> {
    let mut disk_usage = HashMap::new();
    let disks = Disks::new_with_refreshed_list();

    for disk in &disks {
        let mount_point = disk.mount_point().to_string_lossy().to_string();
        let total = disk.total_space();
        let available = disk.available_space();
        let used = total - available;

        disk_usage.insert(mount_point, DiskUsage {
            total,
            used,
            available,
        });
    }

    disk_usage
}

pub fn collect_memory_usage() -> MemoryUsage {
    let mut sys = System::new();
    sys.refresh_memory();

    let total = sys.total_memory();
    let used = sys.used_memory();
    let available = total - used;

    MemoryUsage {
        total,
        used,
        available,
    }
}

pub fn collect_uptime() -> Option<u64> {
    Some(System::uptime())
}

// Extension trait for pipe operations
trait Pipe<T> {
    fn pipe<U, F>(self, f: F) -> U
    where
        F: FnOnce(Self) -> U,
        Self: Sized;
}

impl<T> Pipe<T> for T {
    fn pipe<U, F>(self, f: F) -> U
    where
        F: FnOnce(Self) -> U,
    {
        f(self)
    }
}