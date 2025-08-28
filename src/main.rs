mod cli;
mod system;
mod users;
mod api;
mod ssh_keys;
mod update;

use clap::Parser;
use tracing::{info, error, warn, instrument};
use anyhow::Result;

use cli::Args;
use api::{ApiClient, AgentReport};
use ssh_keys::SshKeyManager;
use update::UpdateManager;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    
    let args = Args::parse();
    
    println!("PubliKey Agent v{}", args.agent_version);
    if let Some(ref endpoint) = args.endpoint {
        println!("Endpoint: {}", endpoint);
    }
    if args.dry_run {
        println!("DRY RUN MODE: No files will be modified");
    }
    
    info!("Starting PubliKey Agent v{}", args.agent_version);
    if let Some(ref endpoint) = args.endpoint {
        info!("Endpoint: {}", endpoint);
    }
    info!("Dry run mode: {}", args.dry_run);
    
    // Validate that include and exclude users are not both specified
    if !args.include_users.is_empty() && !args.exclude_users.is_empty() {
        eprintln!("Error: Cannot specify both --include-users and --exclude-users. Use only one.");
        std::process::exit(1);
    }
    
    // Handle update operations first
    if args.check_update || args.update {
        println!("Checking for updates...");
        let update_manager = UpdateManager::new()?;
        let update_installed = update_manager.check_and_update(&args.agent_version, args.dry_run, args.update).await?;
        
        // If we just installed an update, exit so user can restart with new version
        if args.update && update_installed {
            println!("Please restart the agent to use the new version.");
            return Ok(());
        }
        
        // If we just checked for updates, exit
        if args.check_update && !args.update {
            return Ok(());
        }
        
        // If we were trying to update but no update was needed, exit
        if args.update && !update_installed {
            return Ok(());
        }
    }
    
    // Validate required arguments for normal operations
    let endpoint = args.endpoint.ok_or_else(|| anyhow::anyhow!("--endpoint is required for normal operations"))?;
    let token = args.token.ok_or_else(|| anyhow::anyhow!("--token is required for normal operations"))?;
    
    let api_client = ApiClient::new(endpoint, token)?;
    
    // Initial health check
    println!("Checking API health...");
    match api_client.health_check().await {
        Ok(true) => {
            println!("API health check passed");
            info!("API health check passed");
        },
        Ok(false) => {
            println!("Warning: API health check failed, but continuing...");
            warn!("API health check failed, but continuing...");
        },
        Err(e) => {
            println!("Warning: Health check error: {}, continuing anyway...", e);
            error!("Health check error: {}", e);
            warn!("Continuing despite health check failure...");
        }
    }
    
    println!("Running report...");
    info!("Running report");
    match run_report_cycle(&api_client, &args.agent_version, args.dry_run, &args.exclude_users, &args.include_users, args.user_mode).await {
        Ok(_) => {
            println!("Report completed successfully");
            info!("Report completed successfully");
        }
        Err(e) => {
            let error_msg = e.to_string();
            if error_msg.contains("Agent version") && error_msg.contains("too old") {
                eprintln!("❌ {}", error_msg);
                eprintln!("Please download and install the latest version of the PubliKey agent.");
            } else {
                eprintln!("Error: {}", error_msg);
            }
            return Err(e);
        }
    }
    
    Ok(())
}

#[instrument(skip(api_client, exclude_users, include_users))]
async fn run_report_cycle(api_client: &ApiClient, agent_version: &str, dry_run: bool, exclude_users: &[String], include_users: &[String], user_mode: bool) -> Result<()> {
    info!("Starting report cycle");
    
    // Collect system information
    let hostname = system::collect_hostname()?;
    let system_info = system::collect_system_info()?;
    let users = users::collect_users(exclude_users, include_users, user_mode)?;
    
    println!("Collected system data:");
    println!("  Hostname: {}", hostname);
    println!("  OS: {} {} ({})", system_info.distribution, system_info.version, system_info.arch);
    println!("  Users: {} (filtered: UID 0 and >= 1000)", users.len());
    
    info!("Collected system data:");
    info!("  Hostname: {}", hostname);
    info!("  OS: {} {} ({})", system_info.distribution, system_info.version, system_info.arch);
    info!("  Users: {} (filtered: UID 0 and >= 1000)", users.len());
    
    // Create report
    let report = AgentReport {
        hostname,
        system_info,
        agent_version: agent_version.to_string(),
        users: users.clone(),
    };
    
    // Send report with retry logic
    println!("Sending report to server...");
    let response = api_client.report_with_retry(&report, 3).await?;
    
    println!("Report sent successfully");
    info!("Report sent successfully");
    if let Some(host_id) = &response.host_id {
        println!("Host ID: {}", host_id);
        info!("Host ID: {}", host_id);
    }
    
    // Fetch key assignments and deploy SSH keys
    match api_client.get_key_assignments().await {
        Ok(key_response) => {
            let assignment_count = key_response.assignments.as_ref().map(|a| a.len()).unwrap_or(0);
            println!("Retrieved {} SSH key assignments", assignment_count);
            info!("Retrieved {} SSH key assignments", assignment_count);
            
            if let Some(assignments) = &key_response.assignments {
                let mode = if dry_run { " (DRY RUN)" } else { "" };
                println!("Syncing SSH keys{}...", mode);
                let ssh_manager = SshKeyManager::new();
                
                match ssh_manager.sync_ssh_keys(&users, assignments, dry_run, user_mode) {
                    Ok(stats) => {
                        let prefix = if dry_run { "Would have: " } else { "" };
                        println!("SSH key sync completed{}:", mode);
                        println!("  {} users processed", stats.users_processed);
                        println!("  {}{} keys added", prefix, stats.keys_added);
                        println!("  {}{} keys removed", prefix, stats.keys_removed);
                        println!("  {}{} files updated", prefix, stats.files_updated);
                        if stats.errors > 0 {
                            println!("  {} errors occurred", stats.errors);
                        }
                        
                        info!("SSH key sync stats: {:?}", stats);
                    }
                    Err(e) => {
                        eprintln!("SSH key sync failed: {}", e);
                        error!("SSH key sync failed: {}", e);
                    }
                }
            } else {
                info!("No key assignments to process");
            }
        }
        Err(e) => {
            eprintln!("Failed to fetch key assignments: {}", e);
            error!("Failed to fetch key assignments: {}", e);
        }
    }
    
    Ok(())
}
