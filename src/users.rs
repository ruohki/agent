use serde::Serialize;
use anyhow::Result;

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

pub fn collect_users() -> Result<Vec<UserInfo>> {
    let mut users = Vec::new();
    
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
    
    // Sort by UID for consistent ordering
    users.sort_by_key(|u| u.uid);
    
    Ok(users)
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
        
        // Default shell to /bin/bash if empty or nologin
        let shell = if shell.is_empty() || shell == "/usr/bin/false" || shell == "/sbin/nologin" || shell == "/bin/false" {
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

fn is_user_disabled(shell: &str) -> bool {
    // User is considered disabled if shell is /usr/bin/false or /sbin/nologin
    shell == "/usr/bin/false" || shell == "/sbin/nologin" || shell == "/bin/false"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collect_users() {
        let users = collect_users().unwrap();
        
        // Should have at least root user
        assert!(!users.is_empty());
        
        // Check that all users have valid UIDs (0 or >= 1000)
        for user in &users {
            assert!(user.uid == 0 || user.uid >= 1000);
        }
        
        // Should have root user
        assert!(users.iter().any(|u| u.uid == 0 && u.username == "root"));
    }

    #[test]
    fn test_user_disabled_detection() {
        assert!(is_user_disabled("/usr/bin/false"));
        assert!(is_user_disabled("/sbin/nologin"));
        assert!(!is_user_disabled("/bin/bash"));
        assert!(!is_user_disabled("/bin/zsh"));
    }
}