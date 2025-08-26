# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is the KeyMeister agent (`kmagent`), a Rust-based system monitoring agent that reports host information and manages SSH key deployments. The agent runs once per invocation and communicates with a KeyMeister server via REST API to:

- Report system status, users, and metrics to the server
- Sync SSH key assignments from the server  
- Deploy SSH keys to user authorized_keys files

For continuous monitoring, the agent should be scheduled via systemd timers, cron, or similar scheduling mechanisms.

## Development Commands

```bash
# Build the project
cargo build

# Build for release
cargo build --release

# Run the agent (requires --token and --endpoint)
cargo run -- --token <TOKEN> --endpoint <ENDPOINT>

# Run with verbose logging
RUST_LOG=info cargo run -- --token <TOKEN> --endpoint <ENDPOINT>

# Example with local development server
cargo run -- --token test-token --endpoint http://localhost:3000

# Check for compilation errors
cargo check

# Format code
cargo fmt

# Run linter
cargo clippy

# Run tests
cargo test

# Show help
cargo run -- --help
```

## Architecture

The agent implements a monitoring and key management system with these core responsibilities:

### System Information Collection
- Hostname and system details (OS, arch, kernel, distribution)
- User discovery (UID >= 1000 + root user, filtering system users)
- System metrics (load average, disk usage, memory, uptime)

### API Communication
- Authenticates using Bearer tokens from KeyMeister server
- Primary endpoint: `POST /agent/report` for comprehensive system reporting
- Secondary endpoint: `GET /host/keys` for SSH key assignment retrieval
- Implements error handling with exponential backoff

### SSH Key Management
- Manages `~/.ssh/authorized_keys` files for assigned users
- Creates SSH directories with proper permissions (700 for .ssh, 600 for authorized_keys)
- Validates public key formats before deployment
- Handles both key addition and removal based on server assignments

## API Integration

The agent follows the API specification in `api_documentation.md`:
- Makes single report per execution to maintain server sync
- Filters users appropriately (UID 0 and >= 1000 only)  
- Handles authentication errors and connection failures gracefully
- Logs API interactions for debugging while protecting sensitive token data
- Designed to be run periodically by external schedulers (systemd, cron, etc.)

## Security Considerations

- API tokens must be stored securely (environment variables or protected files)
- SSH key deployments require proper file permissions and ownership
- System user filtering prevents exposure of service accounts
- All API communication should use HTTPS in production
- Uses rustls for TLS (pure Rust implementation, no OpenSSL dependency)