use std::collections::HashMap;
use std::fs::{self, Permissions};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use anyhow::{Result, Context, anyhow};
use tracing::{info, warn, error, debug, instrument};
use serde::Serialize;

use crate::api::KeyAssignment;
use crate::users::UserInfo;

/// Represents a parsed SSH public key
#[derive(Debug, Clone, PartialEq)]
pub struct SshKey {
    pub key_type: String,
    pub key_data: String,
    pub comment: Option<String>,
    pub fingerprint: String,
}

/// Information about an authorized_keys file
#[derive(Debug, Clone)]
pub struct AuthorizedKeysFile {
    pub path: PathBuf,
    pub username: String,
    pub uid: u32,
    pub exists: bool,
}

/// Statistics about SSH key operations
#[derive(Debug, Serialize)]
pub struct KeySyncStats {
    pub users_processed: u32,
    pub keys_added: u32,
    pub keys_removed: u32,
    pub files_updated: u32,
    pub errors: u32,
}

/// SSH key validation and parsing
impl SshKey {
    /// Parse an SSH public key line
    pub fn parse(line: &str) -> Result<Self> {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return Err(anyhow!("Empty or comment line"));
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            return Err(anyhow!("Invalid SSH key format: too few parts"));
        }

        let key_type = parts[0].to_string();
        let key_data = parts[1].to_string();
        let comment = if parts.len() > 2 {
            Some(parts[2..].join(" "))
        } else {
            None
        };

        // Validate key type
        Self::validate_key_type(&key_type)?;
        
        // Validate key data (base64)
        Self::validate_key_data(&key_data)?;

        // Generate fingerprint
        let fingerprint = Self::calculate_fingerprint(&key_type, &key_data)?;

        Ok(SshKey {
            key_type,
            key_data,
            comment,
            fingerprint,
        })
    }

    /// Validate SSH key type
    fn validate_key_type(key_type: &str) -> Result<()> {
        const ALLOWED_KEY_TYPES: &[&str] = &[
            "ssh-rsa",
            "ssh-dss", 
            "ssh-ed25519",
            "ecdsa-sha2-nistp256",
            "ecdsa-sha2-nistp384", 
            "ecdsa-sha2-nistp521",
            "sk-ssh-ed25519@openssh.com",
            "sk-ecdsa-sha2-nistp256@openssh.com",
        ];

        if ALLOWED_KEY_TYPES.contains(&key_type) {
            Ok(())
        } else {
            Err(anyhow!("Unsupported SSH key type: {}", key_type))
        }
    }

    /// Validate base64 key data
    fn validate_key_data(key_data: &str) -> Result<()> {
        use base64::Engine;
        let engine = base64::engine::general_purpose::STANDARD;
        
        engine.decode(key_data)
            .context("Invalid base64 in SSH key data")?;
        
        Ok(())
    }

    /// Calculate SHA256 fingerprint
    fn calculate_fingerprint(_key_type: &str, key_data: &str) -> Result<String> {
        use sha2::{Sha256, Digest};
        use base64::Engine;
        
        let engine = base64::engine::general_purpose::STANDARD;
        let key_bytes = engine.decode(key_data)
            .context("Failed to decode key data for fingerprint")?;
        
        let mut hasher = Sha256::new();
        hasher.update(&key_bytes);
        let hash = hasher.finalize();
        
        // Format as SSH fingerprint
        let fingerprint = engine.encode(&hash);
        Ok(format!("SHA256:{}", fingerprint))
    }

    /// Convert back to SSH public key format
    pub fn to_string(&self) -> String {
        match &self.comment {
            Some(comment) => format!("{} {} {}", self.key_type, self.key_data, comment),
            None => format!("{} {}", self.key_type, self.key_data),
        }
    }

    /// Check if this key matches a PubliKey assignment
    pub fn matches_assignment(&self, assignment: &KeyAssignment) -> bool {
        // Primary match: fingerprint
        if self.fingerprint == assignment.fingerprint {
            return true;
        }
        
        // Secondary match: key type and data
        self.key_type == assignment.key_type && 
        self.key_data == assignment.public_key.split_whitespace().nth(1).unwrap_or("")
    }
}

/// SSH key file management
pub struct SshKeyManager {
    managed_marker: String,
}

impl SshKeyManager {
    pub fn new() -> Self {
        Self {
            managed_marker: "# PubliKey managed - do not edit manually".to_string(),
        }
    }

    /// Discover all authorized_keys files for given users
    pub fn discover_authorized_keys_files(&self, users: &[UserInfo]) -> Result<Vec<AuthorizedKeysFile>> {
        let mut files = Vec::new();
        
        // Get authorized_keys file patterns from sshd_config
        let auth_keys_patterns = self.get_authorized_keys_patterns()?;
        info!("Found {} AuthorizedKeysFile patterns in sshd_config", auth_keys_patterns.len());
        
        for user in users {
            let user_home = if user.uid == 0 {
                PathBuf::from("/root")
            } else {
                match &user.home_dir {
                    Some(home) => PathBuf::from(home),
                    None => PathBuf::from("/home").join(&user.username),
                }
            };
            
            // Expand each pattern for this user
            for pattern in &auth_keys_patterns {
                if let Some(expanded_path) = self.expand_authorized_keys_pattern(pattern, &user.username, &user_home) {
                    let exists = expanded_path.exists();
                    
                    files.push(AuthorizedKeysFile {
                        path: expanded_path,
                        username: user.username.clone(),
                        uid: user.uid,
                        exists,
                    });
                }
            }
        }
        
        info!("Discovered {} authorized_keys files across all patterns", files.len());
        Ok(files)
    }

    /// Parse sshd_config to find AuthorizedKeysFile directives
    fn get_authorized_keys_patterns(&self) -> Result<Vec<String>> {
        let mut patterns = Vec::new();
        
        // Default pattern if no sshd_config found
        let default_patterns = vec![".ssh/authorized_keys".to_string()];
        
        // Common sshd_config locations
        let sshd_config_paths = [
            "/etc/ssh/sshd_config",
            "/etc/sshd_config",
            "/usr/local/etc/ssh/sshd_config",
        ];
        
        let mut found_config = false;
        for config_path in &sshd_config_paths {
            if let Ok(content) = fs::read_to_string(config_path) {
                info!("Reading SSH configuration from: {}", config_path);
                found_config = true;
                
                for line in content.lines() {
                    let line = line.trim();
                    
                    // Skip comments and empty lines
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    
                    // Look for AuthorizedKeysFile directive
                    if let Some(keys_part) = line.strip_prefix("AuthorizedKeysFile") {
                        let keys_part = keys_part.trim();
                        
                        // Handle multiple files separated by spaces
                        for pattern in keys_part.split_whitespace() {
                            if !pattern.is_empty() {
                                patterns.push(pattern.to_string());
                                info!("Found AuthorizedKeysFile pattern: {}", pattern);
                            }
                        }
                    }
                }
                break; // Use first found config file
            }
        }
        
        if !found_config {
            warn!("No sshd_config found, using default authorized_keys location");
            patterns = default_patterns;
        } else if patterns.is_empty() {
            info!("No AuthorizedKeysFile directive found in sshd_config, using default");
            patterns = default_patterns;
        }
        
        Ok(patterns)
    }

    /// Expand SSH authorized_keys file pattern with user-specific values
    fn expand_authorized_keys_pattern(&self, pattern: &str, username: &str, home_dir: &PathBuf) -> Option<PathBuf> {
        let mut expanded = pattern.to_string();
        
        // Replace SSH configuration tokens
        expanded = expanded.replace("%h", &home_dir.to_string_lossy());
        expanded = expanded.replace("%u", username);
        expanded = expanded.replace("%%", "%");
        
        // If pattern starts with /, it's absolute; otherwise relative to home
        let path = if expanded.starts_with('/') {
            PathBuf::from(expanded)
        } else {
            home_dir.join(expanded)
        };
        
        debug!("Expanded pattern '{}' to '{}' for user {}", pattern, path.display(), username);
        Some(path)
    }

    /// Read and parse authorized_keys file
    pub fn read_authorized_keys(&self, file: &AuthorizedKeysFile) -> Result<Vec<SshKey>> {
        if !file.exists {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(&file.path)
            .context(format!("Failed to read {}", file.path.display()))?;

        let mut keys = Vec::new();
        for (line_num, line) in content.lines().enumerate() {
            match SshKey::parse(line) {
                Ok(key) => {
                    debug!("Parsed SSH key on line {}: {}", line_num + 1, key.fingerprint);
                    keys.push(key);
                }
                Err(_) => {
                    // Skip invalid lines (comments, empty lines, malformed keys)
                    debug!("Skipped line {} in {}", line_num + 1, file.path.display());
                }
            }
        }

        info!("Read {} valid SSH keys from {}", keys.len(), file.path.display());
        Ok(keys)
    }

    /// Sync SSH keys for all users based on PubliKey assignments
    #[instrument(skip(self, users, assignments))]
    pub fn sync_ssh_keys(
        &self,
        users: &[UserInfo],
        assignments: &[KeyAssignment],
        dry_run: bool,
        user_mode: bool,
    ) -> Result<KeySyncStats> {
        let mut stats = KeySyncStats {
            users_processed: 0,
            keys_added: 0,
            keys_removed: 0,
            files_updated: 0,
            errors: 0,
        };

        // Group assignments by username
        let mut assignments_by_user: HashMap<String, Vec<&KeyAssignment>> = HashMap::new();
        for assignment in assignments {
            assignments_by_user
                .entry(assignment.username.clone())
                .or_default()
                .push(assignment);
        }

        // Discover all authorized_keys files
        let auth_files = self.discover_authorized_keys_files(users)?;

        for file in &auth_files {
            stats.users_processed += 1;
            
            match self.sync_user_keys(file, assignments_by_user.get(&file.username).unwrap_or(&vec![]), dry_run) {
                Ok(user_stats) => {
                    stats.keys_added += user_stats.keys_added;
                    stats.keys_removed += user_stats.keys_removed;
                    if user_stats.files_updated > 0 {
                        stats.files_updated += 1;
                    }
                }
                Err(e) => {
                    error!("Failed to sync keys for user {}: {}", file.username, e);
                    stats.errors += 1;
                }
            }
        }

        info!(
            "SSH key sync completed: {} users, {} keys added, {} keys removed, {} files updated, {} errors",
            stats.users_processed, stats.keys_added, stats.keys_removed, stats.files_updated, stats.errors
        );

        Ok(stats)
    }

    /// Sync SSH keys for a single user
    fn sync_user_keys(
        &self,
        file: &AuthorizedKeysFile,
        assignments: &[&KeyAssignment],
        dry_run: bool,
    ) -> Result<KeySyncStats> {
        let mut stats = KeySyncStats {
            users_processed: 1,
            keys_added: 0,
            keys_removed: 0,
            files_updated: 0,
            errors: 0,
        };

        // Read existing keys
        let existing_keys = self.read_authorized_keys(file)?;
        
        // Convert assignments to SSH keys
        let mut target_keys = Vec::new();
        for assignment in assignments {
            match self.assignment_to_ssh_key(assignment) {
                Ok(key) => target_keys.push(key),
                Err(e) => {
                    warn!("Invalid key assignment for {}: {}", file.username, e);
                    stats.errors += 1;
                }
            }
        }

        // Determine what changed
        let keys_to_add: Vec<_> = target_keys.iter()
            .filter(|target_key| !existing_keys.iter().any(|existing| existing.fingerprint == target_key.fingerprint))
            .collect();

        let keys_to_remove: Vec<_> = existing_keys.iter()
            .filter(|existing_key| !target_keys.iter().any(|target| target.fingerprint == existing_key.fingerprint))
            .collect();

        // Update statistics
        stats.keys_added = keys_to_add.len() as u32;
        stats.keys_removed = keys_to_remove.len() as u32;

        // If no changes needed, skip file update
        if keys_to_add.is_empty() && keys_to_remove.is_empty() {
            info!("No changes needed for user {}", file.username);
            return Ok(stats);
        }

        // Log changes
        if !keys_to_add.is_empty() {
            let action = if dry_run { "Would add" } else { "Adding" };
            info!("{} {} keys for user {}", action, keys_to_add.len(), file.username);
            for key in &keys_to_add {
                info!("  + {}", key.fingerprint);
            }
        }
        
        if !keys_to_remove.is_empty() {
            let action = if dry_run { "Would remove" } else { "Removing" };
            info!("{} {} keys for user {}", action, keys_to_remove.len(), file.username);
            for key in &keys_to_remove {
                info!("  - {}", key.fingerprint);
            }
        }

        // Write updated authorized_keys file (unless dry run)
        if !dry_run {
            self.write_authorized_keys_file(file, &target_keys)?;
            stats.files_updated = 1;
        } else {
            info!("DRY RUN: Would update {}", file.path.display());
            if nix::unistd::getuid().is_root() {
                let gid = self.get_user_primary_gid(file.uid).map(|g| g.as_raw()).unwrap_or(file.uid);
                info!("DRY RUN: Would set ownership of {} to {}:{}", file.path.display(), file.uid, gid);
            } else if file.uid != nix::unistd::getuid().as_raw() {
                info!("DRY RUN: Would warn about ownership (not running as root)");
            }
            // In dry run, we count it as "would be updated"
            stats.files_updated = 1;
        }

        Ok(stats)
    }

    /// Convert PubliKey assignment to SSH key
    fn assignment_to_ssh_key(&self, assignment: &KeyAssignment) -> Result<SshKey> {
        SshKey::parse(&assignment.public_key)
    }

    /// Write authorized_keys file with proper permissions
    fn write_authorized_keys_file(
        &self,
        file: &AuthorizedKeysFile,
        keys: &[SshKey],
    ) -> Result<()> {
        let ssh_dir = file.path.parent().ok_or_else(|| anyhow!("Invalid authorized_keys path"))?;
        
        // Ensure .ssh directory exists with proper permissions
        if !ssh_dir.exists() {
            info!("Creating SSH directory: {}", ssh_dir.display());
            fs::create_dir_all(ssh_dir)
                .context("Failed to create .ssh directory")?;
        }
        
        // Set SSH directory permissions (700)
        fs::set_permissions(ssh_dir, Permissions::from_mode(0o700))
            .context("Failed to set .ssh directory permissions")?;

        // Create file content
        let mut content = String::new();
        content.push_str(&format!("{}\n", self.managed_marker));
        content.push_str("# This file is managed by PubliKey Agent\n");
        content.push_str("# Manual changes will be overwritten\n\n");

        for key in keys {
            content.push_str(&key.to_string());
            content.push('\n');
        }

        // Write atomically using temporary file
        let temp_path = file.path.with_extension("tmp");
        
        {
            let mut temp_file = fs::File::create(&temp_path)
                .context("Failed to create temporary authorized_keys file")?;
            
            temp_file.write_all(content.as_bytes())
                .context("Failed to write to temporary authorized_keys file")?;
            
            // Set file permissions before moving (600)
            temp_file.set_permissions(Permissions::from_mode(0o600))
                .context("Failed to set temporary file permissions")?;
        }

        // Atomic move
        fs::rename(&temp_path, &file.path)
            .context("Failed to move temporary file to authorized_keys")?;

        // Set proper ownership if running as root
        if nix::unistd::getuid().is_root() {
            let uid = nix::unistd::Uid::from_raw(file.uid);
            // Try to get the primary group for this user, fallback to same ID as UID
            let gid = self.get_user_primary_gid(file.uid).unwrap_or(nix::unistd::Gid::from_raw(file.uid));
            
            // Set ownership of .ssh directory
            if let Err(e) = nix::unistd::chown(ssh_dir, Some(uid), Some(gid)) {
                warn!("Failed to set ownership of {}: {}", ssh_dir.display(), e);
            } else {
                debug!("Set ownership of {} to {}:{}", ssh_dir.display(), file.uid, file.uid);
            }
            
            // Set ownership of authorized_keys file
            if let Err(e) = nix::unistd::chown(&file.path, Some(uid), Some(gid)) {
                warn!("Failed to set ownership of {}: {}", file.path.display(), e);
            } else {
                info!("Set ownership of {} to {}:{}", file.path.display(), file.uid, file.uid);
            }
        } else if file.uid != nix::unistd::getuid().as_raw() {
            warn!("Cannot set ownership of {} to UID {} (not running as root)", 
                  file.path.display(), file.uid);
            warn!("File will be owned by current user ({})", nix::unistd::getuid());
        }

        info!("Updated authorized_keys file: {} ({} keys)", file.path.display(), keys.len());
        Ok(())
    }

    /// Get the primary group ID for a user by looking up /etc/passwd
    fn get_user_primary_gid(&self, uid: u32) -> Option<nix::unistd::Gid> {
        #[cfg(unix)]
        {
            use std::fs;
            
            if let Ok(passwd_content) = fs::read_to_string("/etc/passwd") {
                for line in passwd_content.lines() {
                    if line.trim().is_empty() || line.starts_with('#') {
                        continue;
                    }
                    
                    let parts: Vec<&str> = line.split(':').collect();
                    if parts.len() >= 4 {
                        if let Ok(line_uid) = parts[2].parse::<u32>() {
                            if line_uid == uid {
                                if let Ok(gid) = parts[3].parse::<u32>() {
                                    return Some(nix::unistd::Gid::from_raw(gid));
                                }
                            }
                        }
                    }
                }
            }
        }
        
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_ssh_key() {
        let key_line = "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQDO5XOnOPRhZ/6vQSXnd1QN2i0Swq9FvM3Nwwx5GcBTP9ydZiYqHA00wYRmWoEQpUdrosGE8UaanvdNxCm79oX0AJdiBMm7L73G3J5svovX5jY5ysOB9BnWrMrl+a180L8bWiQ3G/4zMk8dGgkf4NMa6X6KqdfjL0NKKam6q8SJ21CBDaJ5QlBZUEOWsX3qEhs/yswTNT+M7eU+NnaQTzGTfR52sW9ks+lKAF1y4lBiS3L/jeu3eO+XFVVmvbbT6ees+hMnWa0Os8AZx/k9aKao+4GSW1QlQZWuUxcG1r54djP8jiiFrrNsqJ5zEq0R8DkgfOYhxzAfyjAeCaZ6PQuj test@example.com";
        let result = SshKey::parse(key_line);
        assert!(result.is_ok());
        
        let key = result.unwrap();
        assert_eq!(key.key_type, "ssh-rsa");
        assert_eq!(key.key_data, "AAAAB3NzaC1yc2EAAAADAQABAAABAQDO5XOnOPRhZ/6vQSXnd1QN2i0Swq9FvM3Nwwx5GcBTP9ydZiYqHA00wYRmWoEQpUdrosGE8UaanvdNxCm79oX0AJdiBMm7L73G3J5svovX5jY5ysOB9BnWrMrl+a180L8bWiQ3G/4zMk8dGgkf4NMa6X6KqdfjL0NKKam6q8SJ21CBDaJ5QlBZUEOWsX3qEhs/yswTNT+M7eU+NnaQTzGTfR52sW9ks+lKAF1y4lBiS3L/jeu3eO+XFVVmvbbT6ees+hMnWa0Os8AZx/k9aKao+4GSW1QlQZWuUxcG1r54djP8jiiFrrNsqJ5zEq0R8DkgfOYhxzAfyjAeCaZ6PQuj");
        assert_eq!(key.comment, Some("test@example.com".to_string()));
    }

    #[test]
    fn test_parse_key_without_comment() {
        let key_line = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMRzhlN/DHptVc+onPgMzh73YshU9/T3BLEkip0gGx9e";
        let result = SshKey::parse(key_line);
        assert!(result.is_ok());
        
        let key = result.unwrap();
        assert_eq!(key.key_type, "ssh-ed25519");
        assert_eq!(key.comment, None);
    }

    #[test]
    fn test_parse_invalid_key() {
        let invalid_key = "not-a-valid-ssh-key";
        let result = SshKey::parse(invalid_key);
        assert!(result.is_err());
    }

    #[test]
    fn test_ssh_key_to_string() {
        let key = SshKey {
            key_type: "ssh-rsa".to_string(),
            key_data: "AAAAB3NzaC1yc2EAAAADAQABAAABAQDO5XOnOPRhZ/6vQSXnd1QN2i0Swq9FvM3Nwwx5GcBTP9ydZiYqHA00wYRmWoEQpUdrosGE8UaanvdNxCm79oX0AJdiBMm7L73G3J5svovX5jY5ysOB9BnWrMrl+a180L8bWiQ3G/4zMk8dGgkf4NMa6X6KqdfjL0NKKam6q8SJ21CBDaJ5QlBZUEOWsX3qEhs/yswTNT+M7eU+NnaQTzGTfR52sW9ks+lKAF1y4lBiS3L/jeu3eO+XFVVmvbbT6ees+hMnWa0Os8AZx/k9aKao+4GSW1QlQZWuUxcG1r54djP8jiiFrrNsqJ5zEq0R8DkgfOYhxzAfyjAeCaZ6PQuj".to_string(),
            comment: Some("test@example.com".to_string()),
            fingerprint: "SHA256:test".to_string(),
        };
        
        assert_eq!(key.to_string(), "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQDO5XOnOPRhZ/6vQSXnd1QN2i0Swq9FvM3Nwwx5GcBTP9ydZiYqHA00wYRmWoEQpUdrosGE8UaanvdNxCm79oX0AJdiBMm7L73G3J5svovX5jY5ysOB9BnWrMrl+a180L8bWiQ3G/4zMk8dGgkf4NMa6X6KqdfjL0NKKam6q8SJ21CBDaJ5QlBZUEOWsX3qEhs/yswTNT+M7eU+NnaQTzGTfR52sW9ks+lKAF1y4lBiS3L/jeu3eO+XFVVmvbbT6ees+hMnWa0Os8AZx/k9aKao+4GSW1QlQZWuUxcG1r54djP8jiiFrrNsqJ5zEq0R8DkgfOYhxzAfyjAeCaZ6PQuj test@example.com");
    }

    #[test]
    fn test_expand_authorized_keys_pattern() {
        let manager = SshKeyManager::new();
        let username = "testuser";
        let home_dir = PathBuf::from("/home/testuser");

        // Test relative path
        let result = manager.expand_authorized_keys_pattern(".ssh/authorized_keys", username, &home_dir);
        assert_eq!(result, Some(PathBuf::from("/home/testuser/.ssh/authorized_keys")));

        // Test absolute path
        let result = manager.expand_authorized_keys_pattern("/etc/ssh/authorized_keys/%u", username, &home_dir);
        assert_eq!(result, Some(PathBuf::from("/etc/ssh/authorized_keys/testuser")));

        // Test %h expansion
        let result = manager.expand_authorized_keys_pattern("%h/.ssh/authorized_keys", username, &home_dir);
        assert_eq!(result, Some(PathBuf::from("/home/testuser/.ssh/authorized_keys")));

        // Test %u expansion
        let result = manager.expand_authorized_keys_pattern("/var/keys/%u/authorized_keys", username, &home_dir);
        assert_eq!(result, Some(PathBuf::from("/var/keys/testuser/authorized_keys")));

        // Test %% expansion
        let result = manager.expand_authorized_keys_pattern("/path/with%%percent/%u", username, &home_dir);
        assert_eq!(result, Some(PathBuf::from("/path/with%percent/testuser")));
    }
}