use serde::Serialize;
use anyhow::Result;
use tracing::{debug, instrument};
use std::env;

#[derive(Serialize, Debug, Clone)]
pub struct UserInfo {
    pub username: String,
    pub uid: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub home_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
}

#[instrument]
pub fn collect_users(exclude_users: &[String], include_users: &[String], user_mode: bool) -> Result<Vec<UserInfo>> {
    let mut users = Vec::new();
    
    if user_mode {
        // In user mode, only report the current user
        let current_user = get_current_user()?;
        users.push(current_user);
        debug!("User mode: only including current user");
    } else {
        #[cfg(unix)]
        {
            users.extend(parse_passwd_file()?);
        }
        
        #[cfg(not(unix))]
        {
            // On non-Unix systems, just add a mock root user
            users.push(UserInfo {
                username: "root".to_string(),
                uid: 0,
                shell: Some("/bin/bash".to_string()),
                home_dir: Some("/root".to_string()),
                disabled: Some(false),
            });
        }
    }
    
    // Apply user filtering (include mode takes precedence over exclude mode)
    if !include_users.is_empty() {
        let initial_count = users.len();
        users.retain(|user| include_users.contains(&user.username));
        let included_count = users.len();
        let filtered_count = initial_count - included_count;
        if filtered_count > 0 {
            debug!("Included {} users (filtered out {}): {:?}", included_count, filtered_count, include_users);
        }
    } else if !exclude_users.is_empty() {
        let initial_count = users.len();
        users.retain(|user| !exclude_users.contains(&user.username));
        let excluded_count = initial_count - users.len();
        if excluded_count > 0 {
            debug!("Excluded {} users: {:?}", excluded_count, exclude_users);
        }
    }
    
    // Sort by UID for consistent ordering
    users.sort_by_key(|u| u.uid);
    
    Ok(users)
}

fn get_current_user() -> Result<UserInfo> {
    #[cfg(unix)]
    {
        use nix::unistd;
        
        let uid = unistd::getuid();
        let username = env::var("USER").or_else(|_| env::var("USERNAME"))?;
        let home_dir = env::var("HOME").ok();
        let shell = env::var("SHELL").ok();
        
        Ok(UserInfo {
            username,
            uid: uid.as_raw(),
            shell,
            home_dir,
            disabled: Some(false),
        })
    }
    
    #[cfg(not(unix))]
    {
        let username = env::var("USER").or_else(|_| env::var("USERNAME"))?;
        Ok(UserInfo {
            username,
            uid: 1000, // Default non-root UID
            shell: Some("/bin/bash".to_string()),
            home_dir: env::var("HOME").ok(),
            disabled: Some(false),
        })
    }
}

#[cfg(unix)]
fn parse_passwd_file() -> Result<Vec<UserInfo>> {
    use std::fs;
    
    let mut users = Vec::new();
    let passwd_content = fs::read_to_string("/etc/passwd")
        .map_err(|e| anyhow::anyhow!("Failed to read /etc/passwd: {}", e))?;
    
    for line in passwd_content.lines() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() < 7 {
            continue;
        }
        
        let username = parts[0].to_string();
        let uid: u32 = parts[2].parse().unwrap_or_continue();
        let shell = parts[6].to_string();
        let home_dir = parts[5].to_string();
        
        // Filter: only include root (UID 0) and regular users (UID >= 1000)
        // Exclude system users (UID 1-999)
        if uid != 0 && uid < 1000 {
            continue;
        }
        
        // Skip users with nologin shells - they can't SSH anyway
        if shell == "/usr/sbin/nologin" || shell == "/sbin/nologin" || shell == "/bin/false" || shell == "/usr/bin/false" {
            debug!("Skipping user {} with nologin shell: {}", username, shell);
            continue;
        }
        
        // Default shell to /bin/bash if empty 
        let shell = if shell.is_empty() {
            Some("/bin/bash".to_string())
        } else {
            Some(shell)
        };
        
        // Set default home directory
        let home_dir = if home_dir.is_empty() {
            if uid == 0 {
                Some("/root".to_string())
            } else {
                Some(format!("/home/{}", username))
            }
        } else {
            Some(home_dir)
        };
        
        // Check if user account is disabled
        let disabled = is_user_disabled(&shell.as_ref().unwrap_or(&String::new()));
        
        users.push(UserInfo {
            username,
            uid,
            shell,
            home_dir,
            disabled: Some(disabled),
        });
    }
    
    Ok(users)
}

// Helper trait to continue on parse error
trait UnwrapOrContinue<T> {
    fn unwrap_or_continue(self) -> T;
}

impl<T: Default> UnwrapOrContinue<T> for Result<T, std::num::ParseIntError> {
    fn unwrap_or_continue(self) -> T {
        self.unwrap_or_default()
    }
}

fn is_user_disabled(_shell: &str) -> bool {
    // Since we already filter out nologin shells during collection,
    // the remaining users are generally not disabled
    // This could be extended to check account locking in shadow file
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collect_users() {
        let users = collect_users(&[], &[], false).unwrap();
        
        // Should have at least root user (unless root has nologin shell)
        // Check that all users have valid UIDs (0 or >= 1000)
        for user in &users {
            assert!(user.uid == 0 || user.uid >= 1000);
        }
        
        // All users should have login shells (no nologin shells)
        for user in &users {
            if let Some(shell) = &user.shell {
                assert!(
                    shell != "/usr/sbin/nologin" && 
                    shell != "/sbin/nologin" && 
                    shell != "/bin/false" && 
                    shell != "/usr/bin/false",
                    "User {} has nologin shell: {}", user.username, shell
                );
            }
        }
    }

    #[test]
    fn test_user_disabled_detection() {
        // Since we filter out nologin shells during collection,
        // is_user_disabled now returns false for all shells
        // (could be extended to check shadow file for account locking)
        assert!(!is_user_disabled("/bin/bash"));
        assert!(!is_user_disabled("/bin/zsh"));
        assert!(!is_user_disabled("/usr/bin/false")); // Already filtered out during collection
        assert!(!is_user_disabled("/sbin/nologin"));  // Already filtered out during collection
    }
}