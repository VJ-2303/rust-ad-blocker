#!/bin/bash

# Exit on any error
set -e

# Color output for better UX
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Configuration
REPO="VJ-2303/CrabShield"  # ⚠️ CHANGE THIS
BINARY_NAME="CrabShield"
INSTALL_DIR="/usr/local/bin"
ARCHIVE_NAME="CrabShield-linux-x86_64.tar.gz"
DOWNLOAD_URL="https://github.com/${REPO}/releases/latest/download/${ARCHIVE_NAME}"

echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}   CrabShield Installation Script${NC}"
echo -e "${GREEN}========================================${NC}"
echo ""

# Check if running as root (needed for /usr/local/bin)
if [ "$EUID" -ne 0 ]; then
    echo -e "${RED}Error: This script must be run as root${NC}"
    echo "Please run: sudo ./install.sh"
    exit 1
fi

# Detect system architecture
ARCH=$(uname -m)
if [ "$ARCH" != "x86_64" ]; then
    echo -e "${RED}Error: Unsupported architecture: $ARCH${NC}"
    echo "This installer only supports x86_64"
    exit 1
fi

# Check for required commands
for cmd in curl tar; do
    if ! command -v $cmd &> /dev/null; then
        echo -e "${RED}Error: $cmd is not installed${NC}"
        echo "Please install $cmd and try again"
        exit 1
    fi
done

# Create temporary directory
TMP_DIR=$(mktemp -d)
trap "rm -rf $TMP_DIR" EXIT

echo -e "${YELLOW}[1/4]${NC} Downloading latest release..."
if curl -L --fail --progress-bar "$DOWNLOAD_URL" -o "$TMP_DIR/$ARCHIVE_NAME"; then
    echo -e "${GREEN}✓ Download complete${NC}"
else
    echo -e "${RED}✗ Download failed${NC}"
    echo "Please check your internet connection and repository URL"
    exit 1
fi

echo -e "${YELLOW}[2/4]${NC} Extracting archive..."
tar -xzf "$TMP_DIR/$ARCHIVE_NAME" -C "$TMP_DIR"
echo -e "${GREEN}✓ Extraction complete${NC}"

echo -e "${YELLOW}[3/4]${NC} Installing binary..."
mv "$TMP_DIR/$BINARY_NAME" "$INSTALL_DIR/$BINARY_NAME"
chmod +x "$INSTALL_DIR/$BINARY_NAME"
echo -e "${GREEN}✓ Installation complete${NC}"

echo -e "${YELLOW}[4/4]${NC} Verifying installation..."
if command -v $BINARY_NAME &> /dev/null; then
    VERSION=$($BINARY_NAME --version 2>/dev/null || echo "version check not available")
    echo -e "${GREEN}✓ Installation verified${NC}"
    echo ""
    echo -e "${GREEN}========================================${NC}"
    echo -e "${GREEN}   Installation Successful!${NC}"
    echo -e "${GREEN}========================================${NC}"
    echo ""
    echo "Binary location: $INSTALL_DIR/$BINARY_NAME"
    echo "Version: $VERSION"
    echo ""
    echo "You can now run: $BINARY_NAME"
else
    echo -e "${RED}✗ Installation verification failed${NC}"
    exit 1
fi
