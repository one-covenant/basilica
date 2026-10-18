#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
IMAGE_NAME="basilica/validator"
IMAGE_TAG="latest"
EXTRACT_BINARY=true
BUILD_IMAGE=true
RELEASE_MODE=true
FEATURES=""
VERITAS_BINARIES_DIR=""

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
        --veritas-binaries)
            VERITAS_BINARIES_DIR="$2"
            shift 2
            ;;
        --help)
            echo "Usage: $0 [--image-name NAME] [--image-tag TAG] [--no-extract] [--no-image] [--debug] [--features FEATURES] [--veritas-binaries DIR]"
            echo ""
            echo "Options:"
            echo "  --image-name NAME         Docker image name (default: basilica/validator)"
            echo "  --image-tag TAG           Docker image tag (default: latest)"
            echo "  --no-extract              Don't extract binary to local filesystem"
            echo "  --no-image                Skip Docker image creation"
            echo "  --debug                   Build in debug mode"
            echo "  --features FEATURES       Additional cargo features to enable"
            echo "  --veritas-binaries DIR    Directory containing executor-binary and validator-binary"
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

if [[ -n "$VERITAS_BINARIES_DIR" ]]; then
    if [[ ! -d "$VERITAS_BINARIES_DIR" ]]; then
        echo "Error: Veritas binaries directory does not exist: $VERITAS_BINARIES_DIR" >&2
        exit 1
    fi
    VERITAS_BINARIES_DIR="$(cd "$VERITAS_BINARIES_DIR" && pwd -P)"
fi

cd "$PROJECT_ROOT"

# Stage into the exact Docker COPY paths. The default pins both public source
# blobs; custom binaries are accepted only through the explicit override.
VERITAS_BINARY_SOURCE=pinned
if [[ "$BUILD_IMAGE" == true ]]; then
    if [[ -n "$VERITAS_BINARIES_DIR" ]]; then
        ./scripts/validator/stage-veritas.sh --destination ./bin \
            --source-dir "$VERITAS_BINARIES_DIR" --custom
        VERITAS_BINARY_SOURCE=custom
    else
        ./scripts/validator/stage-veritas.sh --destination ./bin
    fi
fi

if [[ "$RELEASE_MODE" == "true" ]]; then
    BUILD_ARGS=(--build-arg BUILD_MODE=release)
else
    BUILD_ARGS=(--build-arg BUILD_MODE=debug)
fi

if [[ -n "$FEATURES" ]]; then
    BUILD_ARGS+=(--build-arg "FEATURES=$FEATURES")
fi


# Pass Bittensor network configuration if set
if [[ -n "${BITTENSOR_NETWORK:-}" ]]; then
    BUILD_ARGS+=(--build-arg "BITTENSOR_NETWORK=$BITTENSOR_NETWORK")
    echo "Building with BITTENSOR_NETWORK=$BITTENSOR_NETWORK"
fi

if [[ -n "${METADATA_CHAIN_ENDPOINT:-}" ]]; then
    BUILD_ARGS+=(--build-arg "METADATA_CHAIN_ENDPOINT=$METADATA_CHAIN_ENDPOINT")
    echo "Building with METADATA_CHAIN_ENDPOINT=$METADATA_CHAIN_ENDPOINT"
fi


if [[ "$BUILD_IMAGE" == "true" ]]; then
    echo "Building Docker image: $IMAGE_NAME:$IMAGE_TAG"

    docker build \
        --platform linux/amd64 \
        --build-arg "VERITAS_BINARY_SOURCE=$VERITAS_BINARY_SOURCE" \
        "${BUILD_ARGS[@]}" \
        -f scripts/validator/Dockerfile \
        -t "$IMAGE_NAME:$IMAGE_TAG" \
        .
    echo "Docker image built successfully"
fi

if [[ "$EXTRACT_BINARY" == "true" ]]; then
    echo "Extracting validator binary..."
    container_id=$(docker create "$IMAGE_NAME:$IMAGE_TAG")
    docker cp "$container_id:/usr/local/bin/basilica-validator" ./basilica-validator
    docker rm "$container_id"
    chmod +x ./basilica-validator
    echo "Binary extracted to: ./basilica-validator"
fi

echo "Build completed successfully!"
