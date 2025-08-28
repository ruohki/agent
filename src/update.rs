use reqwest::Client;
use serde::Deserialize;
use anyhow::{Result, anyhow};
use tracing::{info, instrument};
use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;

#[derive(Deserialize, Debug)]
pub struct GitHubRelease {
    pub tag_name: String,
    pub name: String,
    pub body: String,
    pub draft: bool,
    pub prerelease: bool,
    pub assets: Vec<GitHubAsset>,
}

#[derive(Deserialize, Debug)]
pub struct GitHubAsset {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
    pub content_type: String,
}

pub struct UpdateManager {
    client: Client,
    releases_url: String,
}

impl UpdateManager {
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .user_agent(format!("pkagent/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| anyhow!("Failed to create HTTP client: {}", e))?;

        Ok(Self {
            client,
            releases_url: "https://api.github.com/repos/ruohki/agent/releases/latest".to_string(),
        })
    }

    /// Get the current platform-specific binary name
    pub fn get_current_binary_name() -> String {
        let os = if cfg!(target_os = "linux") {
            "linux"
        } else if cfg!(target_os = "macos") {
            "darwin"
        } else if cfg!(target_os = "windows") {
            "windows"
        } else {
            "unknown"
        };

        let arch = if cfg!(target_arch = "x86_64") {
            "x86_64"
        } else if cfg!(target_arch = "aarch64") {
            "aarch64"
        } else if cfg!(target_arch = "arm") {
            "arm"
        } else {
            "unknown"
        };

        format!("pkagent-{}-{}", os, arch)
    }

    /// Fetch the latest release information from GitHub
    #[instrument(skip(self))]
    pub async fn get_latest_release(&self) -> Result<GitHubRelease> {
        info!("Fetching latest release from GitHub: {}", self.releases_url);
        
        let response = self.client
            .get(&self.releases_url)
            .header("Accept", "application/vnd.github.v3+json")
            .send()
            .await
            .map_err(|e| anyhow!("Failed to fetch release info: {}", e))?;

        if !response.status().is_success() {
            return Err(anyhow!("GitHub API request failed: {}", response.status()));
        }

        let release: GitHubRelease = response
            .json()
            .await
            .map_err(|e| anyhow!("Failed to parse release JSON: {}", e))?;

        info!("Latest release: {} ({})", release.name, release.tag_name);
        Ok(release)
    }

    /// Compare version strings (simple semantic version comparison)
    pub fn is_newer_version(current: &str, latest: &str) -> bool {
        // Remove 'v' prefix if present
        let current_clean = current.strip_prefix('v').unwrap_or(current);
        let latest_clean = latest.strip_prefix('v').unwrap_or(latest);

        // Simple version comparison - split by dots and compare numerically
        let current_parts: Vec<u32> = current_clean
            .split('.')
            .filter_map(|s| s.parse().ok())
            .collect();
        let latest_parts: Vec<u32> = latest_clean
            .split('.')
            .filter_map(|s| s.parse().ok())
            .collect();

        // Pad with zeros if needed
        let max_len = current_parts.len().max(latest_parts.len());
        let mut current_padded = current_parts.clone();
        let mut latest_padded = latest_parts.clone();
        
        current_padded.resize(max_len, 0);
        latest_padded.resize(max_len, 0);

        latest_padded > current_padded
    }

    /// Find the appropriate asset for the current platform
    pub fn find_platform_asset<'a>(&self, release: &'a GitHubRelease) -> Result<&'a GitHubAsset> {
        let binary_name = Self::get_current_binary_name();
        
        release
            .assets
            .iter()
            .find(|asset| asset.name == binary_name)
            .ok_or_else(|| anyhow!("No asset found for platform: {}", binary_name))
    }

    /// Download and install an update
    #[instrument(skip(self, asset))]
    pub async fn download_and_install(&self, asset: &GitHubAsset, dry_run: bool) -> Result<()> {
        let current_exe = env::current_exe()
            .map_err(|e| anyhow!("Failed to get current executable path: {}", e))?;

        info!("Downloading update: {} ({} bytes)", asset.name, asset.size);
        
        if dry_run {
            println!("DRY RUN: Would download {} from {}", asset.name, asset.browser_download_url);
            println!("DRY RUN: Would replace current binary at: {}", current_exe.display());
            return Ok(());
        }

        // Download the new binary
        let response = self.client
            .get(&asset.browser_download_url)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to download update: {}", e))?;

        if !response.status().is_success() {
            return Err(anyhow!("Download failed: {}", response.status()));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| anyhow!("Failed to read download bytes: {}", e))?;

        if bytes.len() as u64 != asset.size {
            return Err(anyhow!(
                "Download size mismatch: expected {}, got {}",
                asset.size,
                bytes.len()
            ));
        }

        // Create backup of current binary
        let backup_path = format!("{}.backup", current_exe.to_string_lossy());
        info!("Creating backup at: {}", backup_path);
        fs::copy(&current_exe, &backup_path)
            .map_err(|e| anyhow!("Failed to create backup: {}", e))?;

        // Write new binary to a temporary file first
        let temp_path = format!("{}.new", current_exe.to_string_lossy());
        fs::write(&temp_path, &bytes)
            .map_err(|e| anyhow!("Failed to write new binary: {}", e))?;

        // Set executable permissions
        let metadata = fs::metadata(&temp_path)
            .map_err(|e| anyhow!("Failed to get temp file metadata: {}", e))?;
        let mut permissions = metadata.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&temp_path, permissions)
            .map_err(|e| anyhow!("Failed to set executable permissions: {}", e))?;

        // Atomically replace the current binary
        fs::rename(&temp_path, &current_exe)
            .map_err(|e| anyhow!("Failed to replace current binary: {}", e))?;

        println!("Update installed successfully!");
        println!("Backup saved to: {}", backup_path);
        info!("Update completed successfully");

        Ok(())
    }

    /// Check for and optionally install updates
    #[instrument(skip(self))]
    pub async fn check_and_update(&self, current_version: &str, dry_run: bool, install: bool) -> Result<bool> {
        let release = self.get_latest_release().await?;

        // Skip draft and prerelease versions
        if release.draft || release.prerelease {
            info!("Skipping draft/prerelease version: {}", release.tag_name);
            println!("Latest release is a draft or prerelease, skipping.");
            return Ok(false);
        }

        println!("Current version: {}", current_version);
        println!("Latest version: {}", release.tag_name);

        if Self::is_newer_version(current_version, &release.tag_name) {
            println!("Update available: {} -> {}", current_version, release.tag_name);
            
            if !install {
                println!("Use --update to install the update");
                return Ok(false);
            }

            let asset = self.find_platform_asset(&release)?;
            println!("Found platform asset: {} ({} bytes)", asset.name, asset.size);

            self.download_and_install(asset, dry_run).await?;
            return Ok(true);
        } else {
            println!("You are running the latest version.");
        }

        Ok(false)
    }
}