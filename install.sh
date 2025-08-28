#!/bin/bash
set -e

# Default values
REPO="ruohki/agent"
TOKEN=""
ENDPOINT=""

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
    *)
      echo "Unknown option $1"
      exit 1
      ;;
  esac
done

if [[ -z "$TOKEN" || -z "$ENDPOINT" ]]; then
    echo "Usage: $0 --token <token> --endpoint <endpoint>"
    exit 1
fi

# Detect architecture
ARCH=$(uname -m)
case $ARCH in
    x86_64)
        BINARY_NAME="kmagent-linux-x86_64"
        ;;
    aarch64)
        BINARY_NAME="kmagent-linux-aarch64"
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
curl -L -o kmagent "$DOWNLOAD_URL"

# Install binary
echo "Installing kmagent to /usr/local/bin..."
sudo chmod +x kmagent
sudo mv kmagent /usr/local/bin/

# Create systemd service
echo "Creating systemd service..."
sudo tee /etc/systemd/system/kmagent.service > /dev/null <<EOF
[Unit]
Description=KM Agent
After=network.target

[Service]
Type=oneshot
ExecStart=/usr/local/bin/kmagent --token=$TOKEN --endpoint=$ENDPOINT
EOF

# Create systemd timer
echo "Creating systemd timer..."
sudo tee /etc/systemd/system/kmagent.timer > /dev/null <<EOF
[Unit]
Description=Run KM Agent every minute
Requires=kmagent.service

[Timer]
OnCalendar=*:*:00
Persistent=true

[Install]
WantedBy=timers.target
EOF

# Enable and start timer
echo "Enabling and starting kmagent timer..."
sudo systemctl daemon-reload
sudo systemctl enable kmagent.timer
sudo systemctl start kmagent.timer

echo "Installation complete! KM Agent will run every minute."
echo "Check status with: sudo systemctl status kmagent.timer"