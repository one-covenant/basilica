#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
IMAGE_NAME="basilica/billing"
IMAGE_TAG="latest"
EXTRACT_BINARY=true
BUILD_IMAGE=true
RELEASE_MODE=true
FEATURES=""

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
        --help)
            echo "Usage: $0 [--image-name NAME] [--image-tag TAG] [--no-extract] [--no-image] [--debug] [--features FEATURES]"
            echo ""
            echo "Options:"
            echo "  --image-name NAME         Docker image name (default: basilica/billing)"
            echo "  --image-tag TAG           Docker image tag (default: latest)"
            echo "  --no-extract              Don't extract binary to local filesystem"
            echo "  --no-image                Skip Docker image creation"
            echo "  --debug                   Build in debug mode"
            echo "  --features FEATURES       Additional cargo features to enable"
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

BUILD_ARGS=""
if [[ "$RELEASE_MODE" == "true" ]]; then
    BUILD_ARGS="--build-arg BUILD_MODE=release"
else
    BUILD_ARGS="--build-arg BUILD_MODE=debug"
fi

if [[ -n "$FEATURES" ]]; then
    BUILD_ARGS="$BUILD_ARGS --build-arg FEATURES=$FEATURES"
fi

# Pass AWS configuration if set
if [[ -n "$AWS_REGION" ]]; then
    BUILD_ARGS="$BUILD_ARGS --build-arg AWS_REGION=$AWS_REGION"
    echo "Building with AWS_REGION=$AWS_REGION"
fi

if [[ -n "$AWS_SECRET_NAME" ]]; then
    BUILD_ARGS="$BUILD_ARGS --build-arg AWS_SECRET_NAME=$AWS_SECRET_NAME"
    echo "Building with AWS_SECRET_NAME=$AWS_SECRET_NAME"
fi

# Pass database configuration if set
if [[ -n "$DATABASE_URL" ]]; then
    BUILD_ARGS="$BUILD_ARGS --build-arg DATABASE_URL=$DATABASE_URL"
    echo "Building with custom DATABASE_URL"
fi

if [[ "$BUILD_IMAGE" == "true" ]]; then
    echo "Building Docker image: $IMAGE_NAME:$IMAGE_TAG"

    docker build \
        --platform linux/amd64 \
        $BUILD_ARGS \
        -f scripts/billing/Dockerfile \
        -t "$IMAGE_NAME:$IMAGE_TAG" \
        .
    echo "Docker image built successfully"
fi

if [[ "$EXTRACT_BINARY" == "true" ]]; then
    echo "Extracting billing binary..."
    container_id=$(docker create "$IMAGE_NAME:$IMAGE_TAG")
    docker cp "$container_id:/usr/local/bin/basilica-billing" ./basilica-billing
    docker rm "$container_id"
    chmod +x ./basilica-billing
    echo "Binary extracted to: ./basilica-billing"
fi

echo "Build completed successfully!"
