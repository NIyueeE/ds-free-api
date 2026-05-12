#!/usr/bin/env bash
set -euo pipefail

CONFIG_PATH=""
PORT="22217"
CONTAINER_NAME="ds-free-api-test"
IMAGE="rust:1.95.0-bookworm"
SKIP_FRONTEND="0"
SKIP_BUILD="0"
NO_CACHE_VOLUMES="0"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --config)
      CONFIG_PATH="$2"
      shift 2
      ;;
    --port)
      PORT="$2"
      shift 2
      ;;
    --name)
      CONTAINER_NAME="$2"
      shift 2
      ;;
    --image)
      IMAGE="$2"
      shift 2
      ;;
    --skip-frontend)
      SKIP_FRONTEND="1"
      shift
      ;;
    --skip-build)
      SKIP_BUILD="1"
      shift
      ;;
    --no-cache-volumes)
      NO_CACHE_VOLUMES="1"
      shift
      ;;
    -h|--help)
      cat <<'EOF'
Usage: scripts/dev-docker-run.sh [options]

Options:
  --config PATH       config.toml path. Default: ./config.toml
  --port PORT         host port. Default: 22217
  --name NAME         container name. Default: ds-free-api-test
  --image IMAGE       Rust image. Default: rust:1.95.0-bookworm
  --skip-frontend     skip bun install/build
  --skip-build        skip cargo build
  --no-cache-volumes  use repo-local cache paths instead of Docker volumes
EOF
      exit 0
      ;;
    *)
      echo "unknown option: $1" >&2
      exit 2
      ;;
  esac
done

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing command: $1" >&2
    exit 1
  fi
}

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

require_command docker
require_command bun

if [[ -z "$CONFIG_PATH" ]]; then
  if [[ -f "$repo_root/config.toml" ]]; then
    CONFIG_PATH="$repo_root/config.toml"
  else
    echo "config not specified and ./config.toml does not exist" >&2
    exit 1
  fi
fi

CONFIG_PATH="$(cd "$(dirname "$CONFIG_PATH")" && pwd)/$(basename "$CONFIG_PATH")"
if [[ ! -f "$CONFIG_PATH" ]]; then
  echo "config not found: $CONFIG_PATH" >&2
  exit 1
fi

if [[ "$SKIP_FRONTEND" != "1" ]]; then
  (
    cd "$repo_root/web"
    export BUN_CONFIG_REGISTRY="https://registry.npmmirror.com"
    bun install --frozen-lockfile
    bun run build
  )
fi

mkdir -p "$repo_root/target"
TMP_CONFIG="$repo_root/target/docker-test-config.toml"
sed \
  -e 's/^host[[:space:]]*=[[:space:]]*"127\.0\.0\.1"/host = "0.0.0.0"/' \
  -e 's/^host[[:space:]]*=[[:space:]]*"localhost"/host = "0.0.0.0"/' \
  "$CONFIG_PATH" > "$TMP_CONFIG"

if [[ "$NO_CACHE_VOLUMES" == "1" ]]; then
  mkdir -p "$repo_root/.docker-cargo-home" "$repo_root/target"
  CARGO_MOUNT="$repo_root/.docker-cargo-home:/usr/local/cargo"
  TARGET_MOUNT="$repo_root/target:/work/target"
else
  CARGO_MOUNT="ds-free-api-cargo-home:/usr/local/cargo"
  TARGET_MOUNT="ds-free-api-target:/work/target"
fi

read -r -d '' BUILD_SCRIPT <<'EOF' || true
set -e
sed -i \
  -e 's|http://deb.debian.org/debian|https://mirrors.tuna.tsinghua.edu.cn/debian|g' \
  -e 's|http://deb.debian.org/debian-security|https://mirrors.tuna.tsinghua.edu.cn/debian-security|g' \
  /etc/apt/sources.list.d/debian.sources
mkdir -p /usr/local/cargo
cat > /usr/local/cargo/config.toml <<'CARGOEOF'
[source.crates-io]
replace-with = "rsproxy"

[source.rsproxy]
registry = "sparse+https://rsproxy.cn/index/"
CARGOEOF
apt-get -o Acquire::Retries=5 update
apt-get -o Acquire::Retries=5 install -y --no-install-recommends cmake ninja-build libclang-dev
/usr/local/cargo/bin/cargo build
EOF

if [[ "$SKIP_BUILD" != "1" ]]; then
  docker run --rm \
    -v "$repo_root:/work" \
    -v "$CARGO_MOUNT" \
    -v "$TARGET_MOUNT" \
    -w /work \
    "$IMAGE" \
    bash -c "$BUILD_SCRIPT"
fi

if docker ps -aq --filter "name=^/${CONTAINER_NAME}$" | grep -q .; then
  docker rm -f "$CONTAINER_NAME" >/dev/null
fi

docker run -d \
  --name "$CONTAINER_NAME" \
  -p "$PORT:22217" \
  -v "$TARGET_MOUNT" \
  -v "$repo_root/web/dist:/work/web/dist:ro" \
  -v "$TMP_CONFIG:/app/config/config.toml:ro" \
  -v ds-free-api-test-data:/app/data \
  -e RUST_LOG=info \
  -e DS_DATA_DIR=/app/data \
  -e DS_CONFIG_PATH=/app/config/config.toml \
  -w /work \
  "$IMAGE" \
  /work/target/debug/ds-free-api >/dev/null

sleep 3
curl -fsS "http://127.0.0.1:$PORT/health"
echo
echo "admin: http://127.0.0.1:$PORT/admin"
echo "logs:  docker logs -f $CONTAINER_NAME"
