# PubliKey Agent API Documentation

This document describes the REST API endpoints that the PubliKey agent should use to communicate with the PubliKey server.

## Base URL
```
http://your-keymeister-server:3000/api
```

## Authentication

All API endpoints require authentication using a Bearer token in the `Authorization` header:

```http
Authorization: Bearer km_abc123def456...
```

The token is generated from the PubliKey web interface when editing a host. Each host has its own unique API token.

## Endpoints

### 1. Health Check (Public)

**Endpoint:** `GET /health`  
**Authentication:** None required  
**Purpose:** Check if the API is running

**Response:**
```json
{
  "status": "ok",
  "timestamp": "2024-01-15T10:30:00.000Z",
  "service": "keymeister-api"
}
```

### 2. Agent Report (Primary Endpoint)

**Endpoint:** `POST /agent/report`  
**Authentication:** Required  
**Purpose:** Single endpoint for the agent to report all host information

This is the primary endpoint that replaces multiple smaller endpoints. The agent should call this endpoint periodically (recommended: every 5-10 minutes) to report system status and users.

#### Request Body

```json
{
  "hostname": "web-server-01.company.com",
  "systemInfo": {
    "os": "Linux",
    "arch": "x86_64", 
    "platform": "linux",
    "kernel": "5.15.0-72-generic",
    "distribution": "Ubuntu",
    "version": "22.04 LTS"
  },
  "agentVersion": "1.0.0",
  "users": [
    {
      "username": "root",
      "uid": 0,
      "shell": "/bin/bash",
      "homeDir": "/root",
      "disabled": false
    },
    {
      "username": "ubuntu",
      "uid": 1000,
      "shell": "/bin/bash", 
      "homeDir": "/home/ubuntu",
      "disabled": false
    },
    {
      "username": "alice",
      "uid": 1001,
      "shell": "/bin/bash",
      "homeDir": "/home/alice", 
      "disabled": false
    }
  ],
  "loadAverage": [0.25, 0.15, 0.10],
  "diskUsage": {
    "/": {
      "total": 107374182400,
      "used": 32212254720,
      "available": 70642253824
    },
    "/home": {
      "total": 536870912000,
      "used": 161061273600,
      "available": 349392568320
    }
  },
  "memoryUsage": {
    "total": 8589934592,
    "used": 3435973836,
    "available": 5153960756
  },
  "uptimeSeconds": 86400
}
```

#### Required Fields

- `hostname` (string): The FQDN or hostname of the system
- `systemInfo` (object): Basic system information
  - `os` (string): Operating system name (e.g., "Linux", "Darwin")
  - `arch` (string): System architecture (e.g., "x86_64", "arm64")  
  - `platform` (string): Platform identifier (e.g., "linux", "darwin")
  - `kernel` (string): Kernel version
  - `distribution` (string): OS distribution (e.g., "Ubuntu", "CentOS", "macOS")
  - `version` (string): OS version (e.g., "22.04 LTS", "13.2")
- `agentVersion` (string): Version of the PubliKey agent
- `users` (array): Array of user objects (see User Object Format below)

#### Optional Fields

- `loadAverage` (array): System load averages [1min, 5min, 15min]
- `diskUsage` (object): Disk usage by mount point with total/used/available bytes
- `memoryUsage` (object): Memory usage with total/used/available bytes  
- `uptimeSeconds` (number): System uptime in seconds

#### User Object Format

**Important:** Only report users with UID >= 1000 and the root user (UID 0). System users with UID 1-999 should be filtered out by the agent.

```json
{
  "username": "alice",
  "uid": 1001,
  "shell": "/bin/bash",
  "homeDir": "/home/alice",
  "disabled": false
}
```

**Required Fields:**
- `username` (string): The username
- `uid` (number): User ID (must be 0 for root or >= 1000 for regular users)

**Optional Fields:**
- `shell` (string): Default login shell (defaults to "/bin/bash" if not provided)
- `homeDir` (string): Home directory path (defaults to "/home/{username}" for regular users, "/root" for root)
- `disabled` (boolean): Whether the user account is disabled (defaults to false)

**Special Root User Handling:**
- If root user (UID 0) is not explicitly reported, the server will automatically add it as disabled
- Root user can be enabled/disabled through the PubliKey web interface

#### Response

**Success (200):**
```json
{
  "success": true,
  "hostId": "abc123def456",
  "message": "Host report processed successfully", 
  "usersProcessed": 3,
  "timestamp": "2024-01-15T10:30:00.000Z"
}
```

**Error (400) - Validation Error:**
```json
{
  "error": "Missing required fields: hostname, systemInfo, agentVersion, users"
}
```

**Error (401) - Unauthorized:**
```json
{
  "error": "Access token required"
}
```

**Error (403) - Invalid/Expired Token:**
```json
{
  "error": "Invalid token"
}
```

### 3. Get SSH Key Assignments

**Endpoint:** `GET /host/keys`  
**Authentication:** Required  
**Purpose:** Get current SSH key assignments for this host

The agent should call this endpoint after reporting to get the latest SSH key assignments that need to be deployed.

#### Response

**Success (200):**
```json
{
  "success": true,
  "hostId": "abc123def456",
  "hostname": "web-server-01.company.com",
  "assignments": [
    {
      "username": "ubuntu",
      "fingerprint": "SHA256:abc123def456...",
      "publicKey": "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAAB...",
      "keyType": "rsa",
      "comment": "alice@workstation",
      "usePrimaryKey": false,
      "assignmentId": "user123-fingerprint456"
    },
    {
      "username": "root", 
      "fingerprint": "SHA256:xyz789abc123...",
      "publicKey": "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIG...",
      "keyType": "ed25519",
      "comment": "bob@laptop",
      "usePrimaryKey": true,
      "assignmentId": "user789-primary"
    }
  ],
  "timestamp": "2024-01-15T10:30:00.000Z"
}
```

## Implementation Guidelines for Agent

### 1. System Information Collection

Use platform-appropriate methods to collect system information:

**Linux:**
- `hostname` command or `/proc/sys/kernel/hostname`
- `uname -a` for kernel and architecture
- `/etc/os-release` for distribution and version
- `/proc/loadavg` for load averages
- `df` command for disk usage
- `/proc/meminfo` for memory usage
- `/proc/uptime` for system uptime

**macOS:**
- `hostname` command
- `uname -a` for kernel and architecture  
- `sw_vers` for version information
- `sysctl` for various system metrics

### 2. User Discovery

**Linux:**
- Parse `/etc/passwd` to get users with UID >= 1000 and root (UID 0)
- Filter out system users (UID 1-999)
- Extract username, UID, shell, and home directory

**macOS:**
- Use `dscl` command: `dscl . list /Users UniqueID`
- Filter for UID >= 1000 and UID 0

### 3. Error Handling

- Implement exponential backoff for failed API requests
- Log all API interactions for debugging
- Continue operation even if optional metrics collection fails
- Gracefully handle network timeouts and connection issues

### 4. Security Considerations

- Store the API token securely (file permissions, environment variables)
- Use HTTPS in production environments
- Validate SSL certificates
- Never log the full API token in plain text

### 5. Recommended Agent Behavior

1. **Startup:** Call `/agent/report` immediately to register the host
2. **Periodic Reporting:** Call `/agent/report` every 5-10 minutes
3. **Key Sync:** After each report, call `/host/keys` to get assignments
4. **Deploy Keys:** Update SSH authorized_keys files based on assignments
5. **Error Recovery:** Retry failed requests with exponential backoff

### 6. SSH Key Deployment

When deploying keys from `/host/keys` response:

1. For each assignment, update the `~/.ssh/authorized_keys` file for the specified `username`
2. Create the `.ssh` directory if it doesn't exist (mode 700)
3. Set proper permissions: authorized_keys file mode 600, owned by the user
4. Handle both addition and removal of keys based on current assignments
5. Validate public key format before adding

## Testing the API

Use curl to test the endpoints:

```bash
# Health check
curl http://localhost:3000/api/health

# Submit agent report
curl -X POST \
  -H "Authorization: Bearer km_your_token_here" \
  -H "Content-Type: application/json" \
  -d @agent_report.json \
  http://localhost:3000/api/agent/report

# Get SSH key assignments  
curl -H "Authorization: Bearer km_your_token_here" \
  http://localhost:3000/api/host/keys
```

Example `agent_report.json`:
```json
{
  "hostname": "test-server",
  "systemInfo": {
    "os": "Linux",
    "arch": "x86_64",
    "platform": "linux", 
    "kernel": "5.15.0-72-generic",
    "distribution": "Ubuntu",
    "version": "22.04 LTS"
  },
  "agentVersion": "1.0.0",
  "users": [
    {
      "username": "root",
      "uid": 0,
      "shell": "/bin/bash",
      "homeDir": "/root"
    },
    {
      "username": "testuser",
      "uid": 1000, 
      "shell": "/bin/bash",
      "homeDir": "/home/testuser"
    }
  ]
}
```