#!/bin/bash
set -e

# Basilica CLI Installation Script
# Usage: curl -sSL https://basilica.ai/install.sh | bash

BINARY_NAME="basilica"
BINARY_URL="https://basilica.ai/releases/latest/basilica"
TEMP_DIR=$(mktemp -d)
TEMP_BINARY="$TEMP_DIR/$BINARY_NAME"

# Determine install directory based on permissions
if [ "$EUID" -eq 0 ] || [ -w "/usr/local/bin" ]; then
    INSTALL_DIR="/usr/local/bin"
    SYSTEM_INSTALL=true
else
    INSTALL_DIR="$HOME/.local/bin"
    SYSTEM_INSTALL=false
fi

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# Show ASCII art
show_logo() {
    echo -e "${CYAN}"
    cat << 'EOF'
 /$$                           /$$ /$$ /$$
| $$                          |__/| $$|__/
| $$$$$$$   /$$$$$$   /$$$$$$$ /$$| $$ /$$  /$$$$$$$  /$$$$$$
| $$__  $$ |____  $$ /$$_____/| $$| $$| $$ /$$_____/ |____  $$
| $$  \ $$  /$$$$$$$|  $$$$$$ | $$| $$| $$| $$        /$$$$$$$
| $$  | $$ /$$__  $$ \____  $$| $$| $$| $$| $$       /$$__  $$
| $$$$$$$/|  $$$$$$$ /$$$$$$$/| $$| $$| $$|  $$$$$$$|  $$$$$$$
|_______/  \_______/|_______/ |__/|__/|__/ \_______/ \_______/

EOF
    echo -e "${NC}"
}

# Print colored output
print_info() {
    echo -e "${GREEN}✓${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}⚠${NC} $1"
}

print_error() {
    echo -e "${RED}✗${NC} $1"
}

print_step() {
    echo -e "${BLUE}→${NC} $1"
}

# Cleanup function
cleanup() {
    rm -rf "$TEMP_DIR"
}
trap cleanup EXIT

# Check and setup installation directory
setup_install_dir() {
    mkdir -p "$INSTALL_DIR"
}

# Detect user's shell profile file
detect_shell_profile() {
    if [ -n "$ZSH_VERSION" ]; then
        echo "$HOME/.zshrc"
    elif [ -n "$BASH_VERSION" ]; then
        if [ -f "$HOME/.bashrc" ]; then
            echo "$HOME/.bashrc"
        else
            echo "$HOME/.bash_profile"
        fi
    elif [ -f "$HOME/.profile" ]; then
        echo "$HOME/.profile"
    else
        echo "$HOME/.bashrc"
    fi
}

# Add directory to PATH in shell profile
add_to_path() {
    local dir="$1"
    local profile_file
    profile_file=$(detect_shell_profile)
    
    # Check if PATH export already exists
    if grep -q "export PATH.*$dir" "$profile_file" 2>/dev/null; then
        return 0
    fi
    
    # Add PATH export to profile
    echo "export PATH=\"$dir:\$PATH\"" >> "$profile_file"
    print_info "Added $dir to PATH in $profile_file"
    return 0
}

# Detect architecture
detect_arch() {
    local arch
    arch=$(uname -m)
    case $arch in
        x86_64)
            echo "amd64"
            ;;
        aarch64|arm64)
            echo "arm64"
            ;;
        *)
            print_error "Unsupported architecture: $arch"
            print_status "Supported architectures: x86_64, aarch64"
            exit 1
            ;;
    esac
}

# Check if command exists
command_exists() {
    command -v "$1" >/dev/null 2>&1
}

# Install dependencies
check_dependencies() {
    if ! command_exists curl && ! command_exists wget; then
        print_error "Please install curl or wget first"
        exit 1
    fi
}

# Download binary
download_binary() {
    local arch
    arch=$(detect_arch)
    local download_url="${BINARY_URL}-linux-${arch}"

    print_step "Downloading Basilica CLI..."

    if command_exists curl; then
        curl -fsSL "$download_url" -o "$TEMP_BINARY"
    else
        wget -q "$download_url" -O "$TEMP_BINARY"
    fi

    if [ ! -f "$TEMP_BINARY" ] || [ ! -s "$TEMP_BINARY" ]; then
        print_error "Download failed"
        exit 1
    fi
}

# Verify binary
verify_binary() {
    chmod +x "$TEMP_BINARY"

    if ! "$TEMP_BINARY" --help >/dev/null 2>&1; then
        print_error "Binary verification failed"
        exit 1
    fi
}

# Install binary
install_binary() {
    print_step "Installing to $INSTALL_DIR..."

    # Backup existing binary if present
    if [ -f "$INSTALL_DIR/$BINARY_NAME" ]; then
        mv "$INSTALL_DIR/$BINARY_NAME" "$INSTALL_DIR/$BINARY_NAME.backup.$(date +%s)"
    fi

    # Install new binary
    mv "$TEMP_BINARY" "$INSTALL_DIR/$BINARY_NAME"
    chmod +x "$INSTALL_DIR/$BINARY_NAME"
}

# Setup PATH if needed
setup_path() {
    if [ "$SYSTEM_INSTALL" = false ] && ! echo "$PATH" | grep -q "$HOME/.local/bin"; then
        if add_to_path "$HOME/.local/bin"; then
            print_info "Run 'source $(detect_shell_profile)' or restart your terminal"
        else
            print_warning "Manually add ~/.local/bin to your PATH:"
            echo "  export PATH=\"\$HOME/.local/bin:\$PATH\""
        fi
        echo
    fi
}

# Show completion message
show_completion() {
    echo
    print_info "Basilica CLI installed successfully!"
    echo
    
    echo "Get started:"
    echo "  basilica login"
    echo "  basilica up <gpu-spec>"
    echo "  basilica exec <uid> \"python train.py\""
    echo "  basilica down <uid>"
    echo
}

# Main installation flow
main() {
    show_logo
    echo "Welcome to the Basilica CLI installer!"
    echo

    setup_install_dir
    check_dependencies
    download_binary
    verify_binary
    install_binary
    setup_path
    show_completion
}

# Run main function
main "$@"
