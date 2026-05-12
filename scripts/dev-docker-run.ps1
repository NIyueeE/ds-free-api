param(
    [string]$ConfigPath = "",
    [int]$Port = 22217,
    [string]$ContainerName = "ds-free-api-test",
    [string]$Image = "rust:1.95.0-bookworm",
    [switch]$SkipFrontend,
    [switch]$SkipBuild,
    [switch]$NoCacheVolumes
)

$ErrorActionPreference = "Stop"

function Resolve-RepoRoot {
    $scriptDir = Split-Path -Parent $PSCommandPath
    return (Resolve-Path (Join-Path $scriptDir "..")).Path
}

function Require-Command {
    param([string]$Name)
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        throw "Missing command: $Name"
    }
}

function To-DockerPath {
    param([string]$Path)
    $resolved = (Resolve-Path -LiteralPath $Path).Path
    return $resolved -replace "\\", "/"
}

$repoRoot = Resolve-RepoRoot
Set-Location $repoRoot

Require-Command "docker"
Require-Command "bun"

if ([string]::IsNullOrWhiteSpace($ConfigPath)) {
    $candidate = Join-Path $repoRoot "config.toml"
    if (Test-Path -LiteralPath $candidate) {
        $ConfigPath = $candidate
    } else {
        throw "ConfigPath is required when ./config.toml does not exist."
    }
}

$ConfigPath = (Resolve-Path -LiteralPath $ConfigPath).Path

if (-not $SkipFrontend) {
    Push-Location (Join-Path $repoRoot "web")
    try {
        $env:BUN_CONFIG_REGISTRY = "https://registry.npmmirror.com"
        bun install --frozen-lockfile
        bun run build
    } finally {
        Pop-Location
    }
}

$tmpConfig = Join-Path $repoRoot "target/docker-test-config.toml"
New-Item -ItemType Directory -Force (Split-Path -Parent $tmpConfig) | Out-Null
(Get-Content -LiteralPath $ConfigPath) `
    -replace '^host\s*=\s*"127\.0\.0\.1"', 'host = "0.0.0.0"' `
    -replace '^host\s*=\s*"localhost"', 'host = "0.0.0.0"' |
    Set-Content -LiteralPath $tmpConfig -Encoding UTF8

$cargoHome = "ds-free-api-cargo-home:/usr/local/cargo"
$targetVolume = "ds-free-api-target:/work/target"
if ($NoCacheVolumes) {
    $cargoCachePath = To-DockerPath (Join-Path $repoRoot ".docker-cargo-home")
    $targetPath = To-DockerPath (Join-Path $repoRoot "target")
    $cargoHome = "${cargoCachePath}:/usr/local/cargo"
    $targetVolume = "${targetPath}:/work/target"
}

$repoPath = To-DockerPath $repoRoot
$webDistPath = To-DockerPath (Join-Path $repoRoot "web/dist")
$tmpConfigPath = To-DockerPath $tmpConfig
$repoMount = "${repoPath}:/work"
$webDistMount = "${webDistPath}:/work/web/dist:ro"
$configMount = "${tmpConfigPath}:/app/config/config.toml:ro"

$buildScript = @'
set -e
sed -i \
  -e 's|http://deb.debian.org/debian|https://mirrors.tuna.tsinghua.edu.cn/debian|g' \
  -e 's|http://deb.debian.org/debian-security|https://mirrors.tuna.tsinghua.edu.cn/debian-security|g' \
  /etc/apt/sources.list.d/debian.sources
mkdir -p /usr/local/cargo
cat > /usr/local/cargo/config.toml <<'EOF'
[source.crates-io]
replace-with = "rsproxy"

[source.rsproxy]
registry = "sparse+https://rsproxy.cn/index/"
EOF
apt-get -o Acquire::Retries=5 update
apt-get -o Acquire::Retries=5 install -y --no-install-recommends cmake ninja-build libclang-dev
/usr/local/cargo/bin/cargo build
'@

if (-not $SkipBuild) {
    docker run --rm `
        -v $repoMount `
        -v $cargoHome `
        -v $targetVolume `
        -w /work `
        $Image `
        bash -c $buildScript
}

$existing = docker ps -aq --filter "name=^/$ContainerName$"
if ($existing) {
    docker rm -f $ContainerName | Out-Null
}

docker run -d `
    --name $ContainerName `
    -p "${Port}:22217" `
    -v $targetVolume `
    -v $webDistMount `
    -v $configMount `
    -v ds-free-api-test-data:/app/data `
    -e RUST_LOG=info `
    -e DS_DATA_DIR=/app/data `
    -e DS_CONFIG_PATH=/app/config/config.toml `
    -w /work `
    $Image `
    /work/target/debug/ds-free-api | Out-Null

Start-Sleep -Seconds 3
$health = Invoke-RestMethod -Uri "http://127.0.0.1:$Port/health" -TimeoutSec 10
Write-Host "health: $($health | ConvertTo-Json -Compress)"
Write-Host "admin:  http://127.0.0.1:$Port/admin"
Write-Host "logs:   docker logs -f $ContainerName"
