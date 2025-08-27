use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "kmagent")]
#[command(about = "KeyMeister Agent - System monitoring and SSH key management")]
#[command(long_about = "KeyMeister Agent - System monitoring and SSH key management

This agent runs once per invocation and reports system status to the KeyMeister server.
For continuous monitoring, set up a systemd timer or cron job to run it periodically.

For verbose logging, set RUST_LOG=info environment variable")]
#[command(version)]
pub struct Args {
    /// API token for authentication
    #[arg(long, env = "KMAGENT_TOKEN")]
    pub token: Option<String>,

    /// Server endpoint (FQDN, e.g., http://localhost:3000)
    #[arg(long, env = "KMAGENT_ENDPOINT")]
    pub endpoint: Option<String>,

    /// Agent version to report
    #[arg(long, default_value = env!("CARGO_PKG_VERSION"))]
    pub agent_version: String,

    /// Dry run mode - show what would be done without making changes
    #[arg(long)]
    pub dry_run: bool,

    /// Check for available updates
    #[arg(long)]
    pub check_update: bool,

    /// Download and install updates if available
    #[arg(long)]
    pub update: bool,

}