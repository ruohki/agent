use serde::Serialize;
use sysinfo::System;
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