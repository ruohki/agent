#!/bin/bash
set -e

# Initialize user mode flag
USER_MODE=false

# Default values
REPO="ruohki/agent"
TOKEN=""
ENDPOINT=""
EXCLUDE_USERS=""
INCLUDE_USERS=""

# Parse arguments
while [[ $# -gt 0 ]]; do
  case $1 in
    --token)
      TOKEN="$2"
      shift 2
      ;;
    --endpoint)
      ENDPOINT="$2"
      shift 2
      ;;
    --exclude-users)
      EXCLUDE_USERS="$2"
      shift 2
      ;;
    --include-users)
      INCLUDE_USERS="$2"
      shift 2
      ;;
    --user)
      USER_MODE=true
      shift
      ;;
    *)
      echo "Unknown option $1"
      exit 1
      ;;
  esac
done

if [[ -z "$TOKEN" || -z "$ENDPOINT" ]]; then
    echo "Usage: $0 --token <token> --endpoint <endpoint> [--exclude-users <user1,user2>] [--include-users <user1,user2>] [--user]"
    exit 1
fi

# Check if running as root (only required for system-wide installation)
if [[ $USER_MODE == false && $EUID -ne 0 ]]; then
    echo "Error: This script must be run as root for system-wide installation (use sudo)"
    echo "For user installation, use: $0 --user [other options]"
    exit 1
fi

# Validate that both include and exclude are not specified
if [[ -n "$INCLUDE_USERS" && -n "$EXCLUDE_USERS" ]]; then
    echo "Error: Cannot specify both --include-users and --exclude-users. Use only one."
    exit 1
fi

# Detect architecture
ARCH=$(uname -m)
case $ARCH in
    x86_64)
        BINARY_NAME="pkagent-linux-x86_64"
        ;;
    aarch64)
        BINARY_NAME="pkagent-linux-aarch64"
        ;;
    *)
        echo "Unsupported architecture: $ARCH"
        exit 1
        ;;
esac

# Get latest release
echo "Getting latest release..."
LATEST_RELEASE=$(curl -s "https://api.github.com/repos/$REPO/releases/latest")
DOWNLOAD_URL=$(echo "$LATEST_RELEASE" | grep -o "https://github.com/$REPO/releases/download/[^\"]*/$BINARY_NAME")

if [[ -z "$DOWNLOAD_URL" ]]; then
    echo "Could not find download URL for $BINARY_NAME"
    exit 1
fi

echo "Downloading $BINARY_NAME..."
curl -L -o pkagent "$DOWNLOAD_URL"

# Install binary
if [[ $USER_MODE == true ]]; then
    # User installation
    INSTALL_DIR="$HOME/.local/bin"
    echo "Installing pkagent to $INSTALL_DIR..."
    mkdir -p "$INSTALL_DIR"
    chmod +x pkagent
    mv pkagent "$INSTALL_DIR/"
    
    # Add to PATH if not already there
    if [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
        echo "Adding $INSTALL_DIR to PATH in ~/.bashrc"
        echo "export PATH=\"\$PATH:$INSTALL_DIR\"" >> ~/.bashrc
        echo "Note: You may need to run 'source ~/.bashrc' or start a new terminal session"
    fi
else
    # System installation
    echo "Installing pkagent to /usr/local/bin..."
    sudo chmod +x pkagent
    sudo mv pkagent /usr/local/bin/
fi

# Set up scheduling
USER_FILTER_ARG=""
if [[ -n "$EXCLUDE_USERS" ]]; then
    USER_FILTER_ARG="--exclude-users=$EXCLUDE_USERS"
elif [[ -n "$INCLUDE_USERS" ]]; then
    USER_FILTER_ARG="--include-users=$INCLUDE_USERS"
fi

if [[ $USER_MODE == true ]]; then
    # User mode: Use cron for scheduling
    BINARY_PATH="$HOME/.local/bin/pkagent"
    CRON_CMD="$BINARY_PATH --token=$TOKEN --endpoint=$ENDPOINT --user-mode $USER_FILTER_ARG"
    
    echo "Setting up cron job for user-mode agent..."
    # Add cron job to run every minute
    (crontab -l 2>/dev/null; echo "* * * * * $CRON_CMD") | crontab -
    
    echo "Installation complete! PubliKey Agent will run every minute in user mode."
    echo "Check cron jobs with: crontab -l"
    echo "View logs with: journalctl --user -f -u cron"
else
    # System mode: Use systemd
    echo "Creating systemd service..."
    
    sudo tee /etc/systemd/system/pkagent.service > /dev/null <<EOF
[Unit]
Description=PubliKey Agent
After=network.target

[Service]
Type=oneshot
ExecStart=/usr/local/bin/pkagent --token=$TOKEN --endpoint=$ENDPOINT $USER_FILTER_ARG
EOF

    # Create systemd timer
    echo "Creating systemd timer..."
    sudo tee /etc/systemd/system/pkagent.timer > /dev/null <<EOF
[Unit]
Description=Run PubliKey Agent every minute
Requires=pkagent.service

[Timer]
OnCalendar=*:*:00
Persistent=true

[Install]
WantedBy=timers.target
EOF

    # Enable and start timer
    echo "Enabling and starting pkagent timer..."
    sudo systemctl daemon-reload
    sudo systemctl enable pkagent.timer
    sudo systemctl start pkagent.timer

    echo "Installation complete! PubliKey Agent will run every minute in system mode."
    echo "Check status with: sudo systemctl status pkagent.timer"
fi