# Requires PowerShell 5.1 or higher

# Configuration
$ErrorActionPreference = "Stop"
$REPO = "VJ-2303/CrabShield"  # ⚠️ CHANGE THIS
$BINARY_NAME = "CrabShield.exe"
$INSTALL_DIR = "$env:ProgramFiles\CrabShield"
$ARCHIVE_NAME = "CrabShield-windows-x86_64.zip"
$DOWNLOAD_URL = "https://github.com/$REPO/releases/latest/download/$ARCHIVE_NAME"

# Color functions
function Write-ColorOutput($ForegroundColor, $Message) {
    $fc = $host.UI.RawUI.ForegroundColor
    $host.UI.RawUI.ForegroundColor = $ForegroundColor
    Write-Output $Message
    $host.UI.RawUI.ForegroundColor = $fc
}

Write-ColorOutput Green "========================================"
Write-ColorOutput Green "   CrabShield Installation Script"
Write-ColorOutput Green "========================================"
Write-Output ""

# Check for Administrator privileges
$currentPrincipal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
if (-not $currentPrincipal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-ColorOutput Red "Error: This script must be run as Administrator"
    Write-Output "Right-click PowerShell and select 'Run as Administrator'"
    exit 1
}

# Check PowerShell version
if ($PSVersionTable.PSVersion.Major -lt 5) {
    Write-ColorOutput Red "Error: PowerShell 5.1 or higher is required"
    Write-Output "Current version: $($PSVersionTable.PSVersion)"
    exit 1
}

# Create temporary directory
$TMP_DIR = Join-Path $env:TEMP "CrabShield-install-$(Get-Random)"
New-Item -ItemType Directory -Path $TMP_DIR | Out-Null

try {
    Write-ColorOutput Yellow "[1/5] Downloading latest release..."
    $ProgressPreference = 'SilentlyContinue'  # Speed up download
    Invoke-WebRequest -Uri $DOWNLOAD_URL -OutFile "$TMP_DIR\$ARCHIVE_NAME" -UseBasicParsing
    Write-ColorOutput Green "✓ Download complete"

    Write-ColorOutput Yellow "[2/5] Extracting archive..."
    Expand-Archive -Path "$TMP_DIR\$ARCHIVE_NAME" -DestinationPath $TMP_DIR -Force
    Write-ColorOutput Green "✓ Extraction complete"

    Write-ColorOutput Yellow "[3/5] Creating installation directory..."
    if (-not (Test-Path $INSTALL_DIR)) {
        New-Item -ItemType Directory -Path $INSTALL_DIR | Out-Null
    }
    Write-ColorOutput Green "✓ Directory created"

    Write-ColorOutput Yellow "[4/5] Installing binary..."
    Copy-Item -Path "$TMP_DIR\$BINARY_NAME" -Destination "$INSTALL_DIR\$BINARY_NAME" -Force
    Write-ColorOutput Green "✓ Installation complete"

    Write-ColorOutput Yellow "[5/5] Adding to PATH..."
    $currentPath = [Environment]::GetEnvironmentVariable("Path", "Machine")
    if ($currentPath -notlike "*$INSTALL_DIR*") {
        [Environment]::SetEnvironmentVariable(
            "Path",
            "$currentPath;$INSTALL_DIR",
            "Machine"
        )
        Write-ColorOutput Green "✓ PATH updated (restart terminal for changes to take effect)"
    } else {
        Write-ColorOutput Green "✓ Already in PATH"
    }

    Write-Output ""
    Write-ColorOutput Green "========================================"
    Write-ColorOutput Green "   Installation Successful!"
    Write-ColorOutput Green "========================================"
    Write-Output ""
    Write-Output "Binary location: $INSTALL_DIR\$BINARY_NAME"
    Write-Output ""
    Write-Output "Next steps:"
    Write-Output "1. Close and reopen your terminal"
    Write-Output "2. Run: CrabShield"
    Write-Output ""

} catch {
    Write-ColorOutput Red "✗ Installation failed: $_"
    exit 1
} finally {
    # Cleanup
    if (Test-Path $TMP_DIR) {
        Remove-Item -Path $TMP_DIR -Recurse -Force
    }
}
