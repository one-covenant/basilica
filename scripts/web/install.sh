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

# Detect user's shell type
detect_shell_type() {
    # Prefer the currently running shell over SHELL environment variable
    local shell_path
    local shell_name
    
    # Try to detect the current running shell first
    shell_path="$(ps -p $$ -o comm= 2>/dev/null || echo "")"
    
    # If ps command fails or returns empty, fall back to SHELL env var
    if [ -z "$shell_path" ]; then
        shell_path="${SHELL:-/bin/bash}"
    fi
    
    # Extract just the shell name
    shell_name="$(basename "$shell_path")"
    
    case "$shell_name" in
        bash) echo "bash" ;;
        zsh) echo "zsh" ;;
        fish) echo "fish" ;;
        sh) echo "bash" ;;  # Treat sh as bash for completion purposes
        *) echo "bash" ;;   # Default to bash if unknown
    esac
}

# Detect user's shell profile file
detect_shell_profile() {
    local shell_type
    shell_type="$(detect_shell_type)"
    
    case "$shell_type" in
        zsh)
            echo "$HOME/.zshrc"
            ;;
        fish)
            echo "$HOME/.config/fish/config.fish"
            ;;
        bash|*)
            if [ -f "$HOME/.bashrc" ]; then
                echo "$HOME/.bashrc"
            elif [ -f "$HOME/.bash_profile" ]; then
                echo "$HOME/.bash_profile"
            else
                echo "$HOME/.profile"
            fi
            ;;
    esac
}

# Add directory to PATH in shell profile
add_to_path() {
    local dir="$1"
    local profile_file
    profile_file=$(detect_shell_profile)

    # Check if PATH export already exists
    if [ -f "$profile_file" ] && grep -q "export PATH.*$dir" "$profile_file" 2>/dev/null; then
        return 0
    fi

    # Check if directory is already in current PATH
    if echo "$PATH" | grep -q "$dir"; then
        return 0
    fi

    # Add PATH export to profile
    if echo "export PATH=\"$dir:\$PATH\"" >> "$profile_file" 2>/dev/null; then
        print_info "Added $dir to PATH"
    else
        print_warning "Could not add to PATH automatically"
        print_info "Please add this to your shell profile: export PATH=\"$dir:\$PATH\""
    fi
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
            print_info "Supported architectures: x86_64, aarch64"
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

    local download_success=false
    local attempts=0
    local max_attempts=3

    while [ $attempts -lt $max_attempts ] && [ "$download_success" = false ]; do
        attempts=$((attempts + 1))

        if command_exists curl; then
            if curl -fsSL "$download_url" -o "$TEMP_BINARY"; then
                download_success=true
            fi
        else
            if wget -q "$download_url" -O "$TEMP_BINARY"; then
                download_success=true
            fi
        fi

        if [ "$download_success" = false ] && [ $attempts -lt $max_attempts ]; then
            print_warning "Download failed, retrying... ($attempts/$max_attempts)"
            sleep 2
        fi
    done

    if [ ! -f "$TEMP_BINARY" ] || [ ! -s "$TEMP_BINARY" ]; then
        print_error "Download failed after $max_attempts attempts"
        print_info "Please check your internet connection and try again"
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

# Check if binary already exists and prompt user
check_existing_installation() {
    if [ -f "$INSTALL_DIR/$BINARY_NAME" ]; then
        echo
        print_warning "Basilica CLI is already installed at $INSTALL_DIR/$BINARY_NAME"

        # Try to get current version
        local current_version
        if current_version=$("$INSTALL_DIR/$BINARY_NAME" --version 2>/dev/null | head -n1); then
            print_info "Current version: $current_version"
        fi

        echo
        # Check if we're in a pipe (common when using curl | bash)
        if [ ! -t 0 ]; then
            print_info "Running in non-interactive mode, proceeding with replacement..."
            print_info "To cancel, press Ctrl+C within 3 seconds..."
            sleep 3
            return 0
        fi

        printf "Do you want to replace it? [y/N]: "
        if read -r response < /dev/tty 2>/dev/null; then
            case "$response" in
                [yY][eE][sS]|[yY])
                    print_info "Proceeding with replacement..."
                    return 0
                    ;;
                *)
                    print_info "Installation cancelled."
                    exit 0
                    ;;
            esac
        else
            # Fallback if /dev/tty is not available
            print_info "Cannot read user input, proceeding with replacement..."
            return 0
        fi
    fi
}

# Backup config if it exists
backup_config() {
    local config_dir="$HOME/.config/basilica"
    local config_file="$config_dir/config.toml"

    if [ -f "$config_file" ]; then
        local backup_file="$config_file.bak.$(date +%s)"
        cp "$config_file" "$backup_file"
        print_info "Config backed up"
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

# Setup shell completions
setup_shell_completions() {
    # Separate declarations from assignments to avoid SC2155
    local shell_type
    local profile_file
    local profile_dir
    shell_type="$(detect_shell_type)"
    profile_file="$(detect_shell_profile)"
    local completion_marker="# Basilica CLI completions"
    
    print_step "Setting up shell completions for $shell_type..."
    
    # Check if completions are already configured
    if [ -f "$profile_file" ] && grep -q "$completion_marker" "$profile_file" 2>/dev/null; then
        print_info "Shell completions already configured"
        return 0
    fi
    
    # Ensure profile file and its directory exist
    profile_dir="$(dirname "$profile_file")"
    if [ ! -d "$profile_dir" ]; then
        mkdir -p "$profile_dir" 2>/dev/null || true
    fi
    
    # Touch the profile file to ensure it exists
    if [ ! -f "$profile_file" ]; then
        touch "$profile_file" 2>/dev/null || true
    fi
    
    # Add completion based on shell type
    local completion_cmd=""
    case "$shell_type" in
        bash)
            completion_cmd='eval "$(COMPLETE=bash basilica)"'
            ;;
        zsh)
            completion_cmd='eval "$(COMPLETE=zsh basilica)"'
            ;;
        fish)
            completion_cmd='COMPLETE=fish basilica | source'
            ;;
        *)
            print_warning "Unknown shell type: $shell_type"
            print_info "Please add shell completions manually for your shell"
            return 1
            ;;
    esac
    
    # Add completion to profile using direct conditional (fixes SC2320)
    if {
        echo ""
        echo "$completion_marker"
        echo "$completion_cmd"
    } >> "$profile_file" 2>/dev/null; then
        print_info "Added shell completions to $profile_file"
        return 0
    else
        print_warning "Could not add completions automatically"
        print_info "Please add this to your $profile_file:"
        echo "  $completion_cmd"
        return 1
    fi
}

# Setup PATH if needed
setup_path() {
    if [ "$SYSTEM_INSTALL" = false ] && ! echo "$PATH" | grep -q "$HOME/.local/bin"; then
        if add_to_path "$HOME/.local/bin"; then
            print_info "PATH updated in $(detect_shell_profile)"
        else
            print_warning "Manually add ~/.local/bin to your PATH:"
            echo "  export PATH=\"\$HOME/.local/bin:\$PATH\""
        fi
        echo
    fi
}

# Show completion message
show_completion() {
    local profile_file="$(detect_shell_profile)"
    
    echo
    print_info "Basilica CLI installed successfully!"
    echo
    
    # Inform about shell completions
    print_info "Shell completions have been configured for tab-completion support"
    print_info "Please restart your terminal or run:"
    echo -e "  ${CYAN}source $profile_file${NC}"
    echo

    echo "Get started:"
    echo "  basilica login                    # Login to Basilica"
    echo "  basilica ls                       # List available GPUs"
    echo "  basilica up                       # Start a GPU rental"
    echo "  basilica exec <uid> \"python train.py\"  # Run your code"
    echo "  basilica down <uid>               # Terminate rental"
    echo
    echo "Use TAB to autocomplete commands and options!"
    echo
}

# Main installation flow
main() {
    show_logo
    echo "Welcome to the Basilica CLI installer!"
    echo

    setup_install_dir
    check_existing_installation
    check_dependencies
    backup_config
    download_binary
    verify_binary
    install_binary
    setup_path
    setup_shell_completions
    show_completion
}

# Run main function
main "$@"
