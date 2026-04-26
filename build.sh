#!/bin/bash
set -e

VERSION="${1:-dev}"

echo "Building alcyone v${VERSION}..."

# 设置版本信息
export APP_VERSION="${VERSION}"
export APP_COMMIT="$(git rev-parse --short HEAD 2>/dev/null || echo 'unknown')"
export APP_BUILD_DATE="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"

# 构建 release 版本
cargo build --release

echo ""
echo "Build completed successfully!"
echo "Binary: target/release/alcyone"
echo "Version: ${APP_VERSION}"
echo "Commit: ${APP_COMMIT}"
echo "Build Date: ${APP_BUILD_DATE}"
