#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
IMAGE_NAME="basilica/cli"
IMAGE_TAG="latest"
BUILD_DOCKER=true
BUILD_NATIVE=false
RELEASE_MODE=true
FEATURES=""
TARGETS=""
OUTPUT_DIR="./build"

# Default targets if none specified
DEFAULT_TARGETS="x86_64-unknown-linux-gnu,aarch64-unknown-linux-gnu,x86_64-apple-darwin,aarch64-apple-darwin"

# Function to print colored output
print_info() {
    echo -e "\033[1;34m[INFO]\033[0m $1"
}

print_success() {
    echo -e "\033[1;32m[SUCCESS]\033[0m $1"
}

print_error() {
    echo -e "\033[1;31m[ERROR]\033[0m $1"
}

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --image-name)
            IMAGE_NAME="$2"
            shift 2
            ;;
        --image-tag)
            IMAGE_TAG="$2"
            shift 2
            ;;
        --no-docker)
            BUILD_DOCKER=false
            shift
            ;;
        --native-only)
            BUILD_NATIVE=true
            BUILD_DOCKER=false
            shift
            ;;
        --debug)
            RELEASE_MODE=false
            shift
            ;;
        --features)
            FEATURES="$2"
            shift 2
            ;;
        --targets)
            TARGETS="$2"
            shift 2
            ;;
        --output-dir)
            OUTPUT_DIR="$2"
            shift 2
            ;;
        --help)
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --image-name NAME         Docker image name (default: basilica/cli)"
            echo "  --image-tag TAG           Docker image tag (default: latest)"
            echo "  --no-docker               Skip Docker image creation"
            echo "  --native-only             Build only for the current platform"
            echo "  --debug                   Build in debug mode"
            echo "  --features FEATURES       Additional cargo features to enable"
            echo "  --targets TARGETS         Comma-separated list of Rust targets to build"
            echo "                            Default: x86_64-unknown-linux-gnu,aarch64-unknown-linux-gnu,"
            echo "                                    x86_64-apple-darwin,aarch64-apple-darwin"
            echo "  --output-dir DIR          Output directory for binaries (default: ./build)"
            echo "  --help                    Show this help message"
            echo ""
            echo "Examples:"
            echo "  # Build for all default platforms"
            echo "  $0"
            echo ""
            echo "  # Build only for macOS"
            echo "  $0 --targets x86_64-apple-darwin,aarch64-apple-darwin --no-docker"
            echo ""
            echo "  # Build only for current platform"
            echo "  $0 --native-only"
            exit 0
            ;;
        *)
            print_error "Unknown option: $1"
            echo "Use --help for usage information"
            exit 1
            ;;
    esac
done

cd "$PROJECT_ROOT"

# Create output directory
mkdir -p "$OUTPUT_DIR"

# Load environment variables from .env if it exists
if [[ -f .env ]]; then
    print_info "Loading environment variables from .env"
    set -a
    source .env
    set +a
fi

# Set default targets if not specified
if [[ -z "$TARGETS" ]]; then
    if [[ "$BUILD_NATIVE" == "true" ]]; then
        # Detect current platform
        CURRENT_ARCH=$(uname -m)
        CURRENT_OS=$(uname -s)
        
        case "$CURRENT_OS" in
            Darwin)
                if [[ "$CURRENT_ARCH" == "x86_64" ]]; then
                    TARGETS="x86_64-apple-darwin"
                elif [[ "$CURRENT_ARCH" == "arm64" ]]; then
                    TARGETS="aarch64-apple-darwin"
                fi
                ;;
            Linux)
                if [[ "$CURRENT_ARCH" == "x86_64" ]]; then
                    TARGETS="x86_64-unknown-linux-gnu"
                elif [[ "$CURRENT_ARCH" == "aarch64" ]]; then
                    TARGETS="aarch64-unknown-linux-gnu"
                fi
                ;;
        esac
        print_info "Building for native platform: $TARGETS"
    else
        TARGETS="$DEFAULT_TARGETS"
    fi
fi

# Function to install Rust target
install_rust_target() {
    local target=$1
    if ! rustup target list --installed | grep -q "$target"; then
        print_info "Installing Rust target: $target"
        rustup target add "$target"
    fi
}

# Function to get binary suffix
get_binary_suffix() {
    local target=$1
    local arch=""
    local os=""
    
    case "$target" in
        x86_64-unknown-linux-gnu)
            echo "linux-amd64"
            ;;
        aarch64-unknown-linux-gnu)
            echo "linux-arm64"
            ;;
        x86_64-apple-darwin)
            echo "darwin-amd64"
            ;;
        aarch64-apple-darwin)
            echo "darwin-arm64"
            ;;
        *)
            echo "$target"
            ;;
    esac
}

# Build cargo command
BUILD_CMD="cargo build"
if [[ "$RELEASE_MODE" == "true" ]]; then
    BUILD_CMD="$BUILD_CMD --release"
    BUILD_DIR="release"
else
    BUILD_DIR="debug"
fi

BUILD_CMD="$BUILD_CMD -p basilica-cli"

if [[ -n "$FEATURES" ]]; then
    BUILD_CMD="$BUILD_CMD --features $FEATURES"
fi

# Build for each target
IFS=',' read -ra TARGET_ARRAY <<< "$TARGETS"
for target in "${TARGET_ARRAY[@]}"; do
    print_info "Building for target: $target"
    
    # Skip macOS builds in Docker (they require native build)
    if [[ "$target" == *"darwin"* ]]; then
        if [[ "$BUILD_DOCKER" == "true" ]] && [[ "$BUILD_NATIVE" == "false" ]]; then
            print_info "Skipping $target (macOS targets require native build)"
            
            # Check if we're on macOS and can build natively
            if [[ "$(uname -s)" == "Darwin" ]]; then
                print_info "Building $target natively on macOS..."
                
                # Install target
                install_rust_target "$target"
                
                # Set environment variables for cross-compilation if needed
                if [[ "$target" == "x86_64-apple-darwin" ]] && [[ "$(uname -m)" == "arm64" ]]; then
                    export CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER=x86_64-apple-darwin14-clang
                    export CC=x86_64-apple-darwin14-clang
                elif [[ "$target" == "aarch64-apple-darwin" ]] && [[ "$(uname -m)" == "x86_64" ]]; then
                    export CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER=aarch64-apple-darwin14-clang
                    export CC=aarch64-apple-darwin14-clang
                fi
                
                # Build
                eval "$BUILD_CMD --target $target"
                
                # Copy binary
                BINARY_NAME="basilica-$(get_binary_suffix $target)"
                cp "target/$target/$BUILD_DIR/basilica" "$OUTPUT_DIR/$BINARY_NAME"
                chmod +x "$OUTPUT_DIR/$BINARY_NAME"
                print_success "Binary built: $OUTPUT_DIR/$BINARY_NAME"
            else
                print_info "Cannot build macOS targets on non-macOS system"
            fi
            continue
        fi
    fi
    
    # Linux targets can be built with Docker or natively
    if [[ "$target" == *"linux"* ]]; then
        if [[ "$BUILD_DOCKER" == "true" ]]; then
            # Use existing Docker build for Linux targets
            print_info "Building $target using Docker..."
            
            # Extract architecture for Docker platform
            if [[ "$target" == "x86_64-unknown-linux-gnu" ]]; then
                DOCKER_ARCH="amd64"
            elif [[ "$target" == "aarch64-unknown-linux-gnu" ]]; then
                DOCKER_ARCH="arm64"
            fi
            
            # Use the existing build.sh script for Docker builds
            "$SCRIPT_DIR/build.sh" \
                --image-name "$IMAGE_NAME" \
                --image-tag "$IMAGE_TAG" \
                --architectures "$DOCKER_ARCH" \
                --single-arch
            
            # Move the binary to output directory with proper naming
            BINARY_NAME="basilica-$(get_binary_suffix $target)"
            if [[ -f "./basilica-linux-$DOCKER_ARCH" ]]; then
                mv "./basilica-linux-$DOCKER_ARCH" "$OUTPUT_DIR/$BINARY_NAME"
                print_success "Binary built: $OUTPUT_DIR/$BINARY_NAME"
            elif [[ -f "./basilica" ]]; then
                mv "./basilica" "$OUTPUT_DIR/$BINARY_NAME"
                print_success "Binary built: $OUTPUT_DIR/$BINARY_NAME"
            fi
            continue
        fi
    fi
    
    # Native build for any target
    if [[ "$BUILD_NATIVE" == "true" ]] || [[ "$BUILD_DOCKER" == "false" ]]; then
        print_info "Building $target natively..."
        
        # Install target
        install_rust_target "$target"
        
        # Build
        eval "$BUILD_CMD --target $target"
        
        # Copy binary
        BINARY_NAME="basilica-$(get_binary_suffix $target)"
        cp "target/$target/$BUILD_DIR/basilica" "$OUTPUT_DIR/$BINARY_NAME"
        chmod +x "$OUTPUT_DIR/$BINARY_NAME"
        print_success "Binary built: $OUTPUT_DIR/$BINARY_NAME"
    fi
done

# Create a summary
print_info "Build complete! Binaries created in $OUTPUT_DIR:"
ls -la "$OUTPUT_DIR"/basilica-* 2>/dev/null || print_error "No binaries found in $OUTPUT_DIR"

# Generate checksums
if command -v sha256sum &> /dev/null; then
    print_info "Generating checksums..."
    cd "$OUTPUT_DIR"
    sha256sum basilica-* > SHA256SUMS 2>/dev/null || true
    cd - > /dev/null
elif command -v shasum &> /dev/null; then
    print_info "Generating checksums..."
    cd "$OUTPUT_DIR"
    shasum -a 256 basilica-* > SHA256SUMS 2>/dev/null || true
    cd - > /dev/null
fi

print_success "Build process completed!"
</parameter>
</invoke>