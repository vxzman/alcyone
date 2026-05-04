#!/usr/bin/env bash
set -e

VERSION="${1:-dev}"
PROFILE="${2:-debug}"

echo "Building alcyone v${VERSION} (${PROFILE})..."

# 强制重新编译 ifaddr6 (C 代码变更不会被 cargo 自动检测)
touch crates/ifaddr6/build.rs 2>/dev/null || true

# 设置版本信息 (通过 build.rs 注入到二进制)
export APP_VERSION="${VERSION}"
export APP_COMMIT="$(git rev-parse --short HEAD 2>/dev/null || echo 'unknown')"
export APP_BUILD_DATE="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"

# 构建
if [ "$PROFILE" = "release" ]; then
    cargo build --release
    BINARY="target/release/alcyone"
else
    cargo build
    BINARY="target/debug/alcyone"
fi

echo ""
echo "Build completed successfully!"
echo "Binary: ${BINARY}"
echo "Version: ${VERSION}"
echo "Commit: ${APP_COMMIT}"
echo "Build Date: ${APP_BUILD_DATE}"
