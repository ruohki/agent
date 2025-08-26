mod cli;
mod system;
mod users;
mod api;

use clap::Parser;
use log::{info, error, warn};
use anyhow::Result;

use cli::Args;
use api::{ApiClient, AgentReport};

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    
    let args = Args::parse();
    
    println!("KeyMeister Agent v{}", args.agent_version);
    println!("Endpoint: {}", args.endpoint);
    
    info!("Starting KeyMeister Agent v{}", args.agent_version);
    info!("Endpoint: {}", args.endpoint);
    
    let api_client = ApiClient::new(args.endpoint.clone(), args.token.clone())?;
    
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
    match run_report_cycle(&api_client, &args.agent_version).await {
        Ok(_) => {
            println!("Report completed successfully");
            info!("Report completed successfully");
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            return Err(e);
        }
    }
    
    Ok(())
}

async fn run_report_cycle(api_client: &ApiClient, agent_version: &str) -> Result<()> {
    info!("Starting report cycle");
    
    // Collect system information
    let hostname = system::collect_hostname()?;
    let system_info = system::collect_system_info()?;
    let users = users::collect_users()?;
    let load_average = system::collect_load_average();
    let disk_usage = Some(system::collect_disk_usage());
    let memory_usage = Some(system::collect_memory_usage());
    let uptime_seconds = system::collect_uptime();
    
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
        users,
        load_average,
        disk_usage,
        memory_usage,
        uptime_seconds,
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
    
    // Fetch key assignments
    match api_client.get_key_assignments().await {
        Ok(key_response) => {
            let assignment_count = key_response.assignments.as_ref().map(|a| a.len()).unwrap_or(0);
            println!("Retrieved {} SSH key assignments", assignment_count);
            println!("{:?}", key_response);
            // TODO: Implement SSH key deployment
            if assignment_count > 0 {
                println!("SSH key deployment not yet implemented");
            }
        }
        Err(e) => {
            error!("Failed to fetch key assignments: {}", e);
        }
    }
    
    Ok(())
}
