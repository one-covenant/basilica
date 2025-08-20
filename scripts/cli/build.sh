#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
IMAGE_NAME="basilica/cli"
IMAGE_TAG="latest"
EXTRACT_BINARY=true
BUILD_IMAGE=true
RELEASE_MODE=true
FEATURES=""
ARCHITECTURES="amd64,arm64"
MULTI_ARCH=true

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
        --no-extract)
            EXTRACT_BINARY=false
            shift
            ;;
        --no-image)
            BUILD_IMAGE=false
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
        --architectures)
            ARCHITECTURES="$2"
            shift 2
            ;;
        --single-arch)
            MULTI_ARCH=false
            shift
            ;;
        --help)
            echo "Usage: $0 [--image-name NAME] [--image-tag TAG] [--no-extract] [--no-image] [--debug] [--features FEATURES] [--architectures ARCHS] [--single-arch]"
            echo ""
            echo "Options:"
            echo "  --image-name NAME         Docker image name (default: basilica/cli)"
            echo "  --image-tag TAG           Docker image tag (default: latest)"
            echo "  --no-extract              Don't extract binary to local filesystem"
            echo "  --no-image                Skip Docker image creation"
            echo "  --debug                   Build in debug mode"
            echo "  --features FEATURES       Additional cargo features to enable"
            echo "  --architectures ARCHS     Comma-separated list of architectures (default: amd64,arm64)"
            echo "  --single-arch             Build for single architecture only (linux/amd64)"
            echo "  --help                    Show this help message"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            echo "Use --help for usage information"
            exit 1
            ;;
    esac
done

cd "$PROJECT_ROOT"

# Load environment variables from .env if it exists
if [[ -f .env ]]; then
    echo "Loading environment variables from .env"
    set -a
    source .env
    set +a
fi

BUILD_ARGS=""
if [[ "$RELEASE_MODE" == "true" ]]; then
    BUILD_ARGS="--build-arg BUILD_MODE=release"
else
    BUILD_ARGS="--build-arg BUILD_MODE=debug"
fi

if [[ -n "$FEATURES" ]]; then
    BUILD_ARGS="$BUILD_ARGS --build-arg FEATURES=$FEATURES"
fi

# Pass Bittensor network configuration if set
if [[ -n "$BITTENSOR_NETWORK" ]]; then
    BUILD_ARGS="$BUILD_ARGS --build-arg BITTENSOR_NETWORK=$BITTENSOR_NETWORK"
    echo "Building with BITTENSOR_NETWORK=$BITTENSOR_NETWORK"
fi

if [[ -n "$METADATA_CHAIN_ENDPOINT" ]]; then
    BUILD_ARGS="$BUILD_ARGS --build-arg METADATA_CHAIN_ENDPOINT=$METADATA_CHAIN_ENDPOINT"
    echo "Building with METADATA_CHAIN_ENDPOINT=$METADATA_CHAIN_ENDPOINT"
fi

# Pass Auth0 configuration if set
if [[ -n "$BASILICA_AUTH0_CLIENT_ID" ]]; then
    BUILD_ARGS="$BUILD_ARGS --build-arg BASILICA_AUTH0_CLIENT_ID=$BASILICA_AUTH0_CLIENT_ID"
fi

if [[ -n "$BASILICA_AUTH0_AUDIENCE" ]]; then
    BUILD_ARGS="$BUILD_ARGS --build-arg BASILICA_AUTH0_AUDIENCE=$BASILICA_AUTH0_AUDIENCE"
fi

if [[ -n "$BASILICA_AUTH0_ISSUER" ]]; then
    BUILD_ARGS="$BUILD_ARGS --build-arg BASILICA_AUTH0_ISSUER=$BASILICA_AUTH0_ISSUER"
fi

if [[ -n "$BASILICA_AUTH0_DOMAIN" ]]; then
    BUILD_ARGS="$BUILD_ARGS --build-arg BASILICA_AUTH0_DOMAIN=$BASILICA_AUTH0_DOMAIN"
fi

if [[ "$BUILD_IMAGE" == "true" ]]; then
    if [[ "$MULTI_ARCH" == "true" ]]; then
        echo "Building multi-architecture Docker images for: $ARCHITECTURES"

        # Convert comma-separated architectures to platform list
        PLATFORMS=""
        IFS=',' read -ra ARCH_ARRAY <<< "$ARCHITECTURES"
        for arch in "${ARCH_ARRAY[@]}"; do
            if [[ -n "$PLATFORMS" ]]; then
                PLATFORMS="$PLATFORMS,linux/$arch"
            else
                PLATFORMS="linux/$arch"
            fi
        done

        # Create multi-arch builder if it doesn't exist
        if ! docker buildx ls | grep -q "basilica-builder"; then
            echo "Creating Docker buildx builder..."
            docker buildx create --name basilica-builder --use
        else
            docker buildx use basilica-builder
        fi

        docker buildx build \
            --platform "$PLATFORMS" \
            $BUILD_ARGS \
            -f scripts/cli/Dockerfile \
            -t "$IMAGE_NAME:$IMAGE_TAG" \
            --load \
            .
        echo "Multi-architecture Docker images built successfully"
    else
        echo "Building Docker image: $IMAGE_NAME:$IMAGE_TAG"

        docker build \
            --platform linux/amd64 \
            $BUILD_ARGS \
            -f scripts/cli/Dockerfile \
            -t "$IMAGE_NAME:$IMAGE_TAG" \
            .
        echo "Docker image built successfully"
    fi
fi

if [[ "$EXTRACT_BINARY" == "true" ]]; then
    if [[ "$MULTI_ARCH" == "true" ]]; then
        echo "Extracting binaries for multiple architectures..."

        IFS=',' read -ra ARCH_ARRAY <<< "$ARCHITECTURES"
        for arch in "${ARCH_ARRAY[@]}"; do
            echo "Extracting binary for $arch..."

            # Create architecture-specific image tag
            arch_image="$IMAGE_NAME:$IMAGE_TAG-$arch"

            # Build single-arch image for extraction
            docker buildx build \
                --platform "linux/$arch" \
                $BUILD_ARGS \
                -f scripts/cli/Dockerfile \
                -t "$arch_image" \
                --load \
                .

            # Extract binary with architecture suffix
            container_id=$(docker create "$arch_image")
            docker cp "$container_id:/usr/local/bin/basilica" "./basilica-linux-$arch"
            docker rm "$container_id"
            chmod +x "./basilica-linux-$arch"
            echo "Binary extracted to: ./basilica-linux-$arch"

            # Clean up architecture-specific image
            docker rmi "$arch_image" 2>/dev/null || true
        done
    else
        echo "Extracting basilica binary..."
        container_id=$(docker create "$IMAGE_NAME:$IMAGE_TAG")
        docker cp "$container_id:/usr/local/bin/basilica" ./basilica
        docker rm "$container_id"
        chmod +x ./basilica
        echo "Binary extracted to: ./basilica"
    fi
fi

echo "Build completed successfully!"
