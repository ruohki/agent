use reqwest::Client;
use serde::{Deserialize, Serialize};
use anyhow::{Result, anyhow};
use tracing::{info, warn, error, instrument};

use crate::system::SystemInfo;
use crate::users::UserInfo;

#[derive(Serialize, Debug)]
pub struct AgentReport {
    pub hostname: String,
    #[serde(rename = "systemInfo")]
    pub system_info: SystemInfo,
    #[serde(rename = "agentVersion")]
    pub agent_version: String,
    pub users: Vec<UserInfo>,
}

#[derive(Deserialize, Debug)]
pub struct AgentReportResponse {
    pub success: bool,
    #[serde(rename = "hostId")]
    pub host_id: Option<String>,
    pub message: Option<String>,
    #[serde(rename = "usersProcessed")]
    pub users_processed: Option<u32>,
    pub timestamp: Option<String>,
    pub error: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct KeyAssignment {
    pub username: String,
    pub fingerprint: String,
    #[serde(rename = "publicKey")]
    pub public_key: String,
    #[serde(rename = "keyType")]
    pub key_type: String,
    pub comment: Option<String>,
    #[serde(rename = "usePrimaryKey")]
    pub use_primary_key: Option<bool>,
    #[serde(rename = "assignmentId")]
    pub assignment_id: String,
}

#[derive(Deserialize, Debug)]
pub struct KeyAssignmentsResponse {
    pub success: bool,
    #[serde(rename = "hostId")]
    pub host_id: Option<String>,
    pub hostname: Option<String>,
    pub assignments: Option<Vec<KeyAssignment>>,
    pub timestamp: Option<String>,
    pub error: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct VersionErrorResponse {
    pub error: String,
    pub message: String,
    #[serde(rename = "minimumVersion")]
    pub minimum_version: String,
    #[serde(rename = "currentVersion")]
    pub current_version: String,
}

pub struct ApiClient {
    client: Client,
    base_url: String,
    token: String,
}

impl ApiClient {
    pub fn new(endpoint: String, token: String) -> Result<Self> {
        let base_url = if endpoint.ends_with('/') {
            format!("{}api", endpoint)
        } else {
            format!("{}/api", endpoint)
        };

        let client = Client::builder()
            .user_agent(format!("kmagent/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| anyhow!("Failed to create HTTP client: {}", e))?;

        Ok(Self {
            client,
            base_url,
            token,
        })
    }

    #[instrument(skip(self))]
    pub async fn health_check(&self) -> Result<bool> {
        let url = format!("{}/health", self.base_url);
        
        info!("Checking API health at: {}", url);
        
        let response = self.client
            .get(&url)
            .send()
            .await
            .map_err(|e| anyhow!("Health check request failed: {}", e))?;

        let status = response.status();
        if status.is_success() {
            info!("Health check passed");
            Ok(true)
        } else {
            warn!("Health check failed with status: {}", status);
            Ok(false)
        }
    }

    #[instrument(skip(self, report))]
    pub async fn report_agent_data(&self, report: &AgentReport) -> Result<AgentReportResponse> {
        let url = format!("{}/agent/report", self.base_url);
        
        info!("Reporting agent data to: {}", url);
        info!("Report contains {} users", report.users.len());
        
        let response = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Content-Type", "application/json")
            .json(report)
            .send()
            .await
            .map_err(|e| anyhow!("Agent report request failed: {}", e))?;

        let status = response.status();
        let response_text = response.text().await
            .map_err(|e| anyhow!("Failed to read response: {}", e))?;

        if status.is_success() {
            let parsed_response: AgentReportResponse = serde_json::from_str(&response_text)
                .map_err(|e| anyhow!("Failed to parse successful response: {}", e))?;
            
            info!("Agent report successful: {}", parsed_response.message.as_deref().unwrap_or("No message"));
            if let Some(users_processed) = parsed_response.users_processed {
                info!("Users processed: {}", users_processed);
            }
            
            Ok(parsed_response)
        } else if status == reqwest::StatusCode::UPGRADE_REQUIRED {
            // Handle HTTP 426 - Agent version too old
            if let Ok(version_error) = serde_json::from_str::<VersionErrorResponse>(&response_text) {
                error!("Agent version too old: {}", version_error.message);
                error!("Current version: {}, Minimum required: {}", 
                       version_error.current_version, version_error.minimum_version);
                return Err(anyhow!("Agent version {} is too old. Minimum required version: {}. Please update the agent.",
                                 version_error.current_version, version_error.minimum_version));
            } else {
                error!("Agent version check failed with HTTP 426 but could not parse response");
                return Err(anyhow!("Agent version too old. Please update the agent."));
            }
        } else {
            // Try to parse as error response first
            if let Ok(error_response) = serde_json::from_str::<AgentReportResponse>(&response_text) {
                if let Some(error_msg) = &error_response.error {
                    error!("API error ({}): {}", status, error_msg);
                    return Err(anyhow!("API request failed: {}", error_msg));
                }
            }
            
            error!("HTTP error ({}): {}", status, response_text);
            Err(anyhow!("HTTP error ({}): {}", status, response_text))
        }
    }

    #[instrument(skip(self))]
    pub async fn get_key_assignments(&self) -> Result<KeyAssignmentsResponse> {
        let url = format!("{}/host/keys", self.base_url);
        
        info!("Fetching key assignments from: {}", url);
        
        let response = self.client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .send()
            .await
            .map_err(|e| anyhow!("Key assignments request failed: {}", e))?;

        let status = response.status();
        let response_text = response.text().await
            .map_err(|e| anyhow!("Failed to read response: {}", e))?;

        if status.is_success() {
            let parsed_response: KeyAssignmentsResponse = serde_json::from_str(&response_text)
                .map_err(|e| anyhow!("Failed to parse key assignments response: {}", e))?;
            
            let assignment_count = parsed_response.assignments.as_ref().map(|a| a.len()).unwrap_or(0);
            info!("Retrieved {} key assignments", assignment_count);
            
            Ok(parsed_response)
        } else {
            // Try to parse as error response first
            if let Ok(error_response) = serde_json::from_str::<KeyAssignmentsResponse>(&response_text) {
                if let Some(error_msg) = &error_response.error {
                    error!("API error ({}): {}", status, error_msg);
                    return Err(anyhow!("API request failed: {}", error_msg));
                }
            }
            
            error!("HTTP error ({}): {}", status, response_text);
            Err(anyhow!("HTTP error ({}): {}", status, response_text))
        }
    }

    #[instrument(skip(self, report))]
    pub async fn report_with_retry(&self, report: &AgentReport, max_retries: u32) -> Result<AgentReportResponse> {
        let mut last_error = None;
        
        for attempt in 1..=max_retries {
            match self.report_agent_data(report).await {
                Ok(response) => return Ok(response),
                Err(e) => {
                    let error_msg = e.to_string();
                    
                    // Don't retry on version errors (HTTP 426) - these won't resolve with retries
                    if error_msg.contains("Agent version") && error_msg.contains("too old") {
                        error!("Version error detected - not retrying: {}", error_msg);
                        return Err(e);
                    }
                    
                    warn!("Report attempt {} failed: {}", attempt, e);
                    last_error = Some(e);
                    
                    if attempt < max_retries {
                        let delay = std::time::Duration::from_secs(2u64.pow(attempt - 1));
                        info!("Retrying in {:?}...", delay);
                        tokio::time::sleep(delay).await;
                    }
                }
            }
        }
        
        Err(last_error.unwrap_or_else(|| anyhow!("All retry attempts failed")))
    }
}