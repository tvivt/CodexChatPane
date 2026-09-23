$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $projectRoot

& npm.cmd ci
if ($LASTEXITCODE -ne 0) { throw "npm ci failed: $LASTEXITCODE" }
& node tests/frontend.mjs
if ($LASTEXITCODE -ne 0) { throw "Frontend checks failed: $LASTEXITCODE" }
& node tests/dynamic.mjs
if ($LASTEXITCODE -ne 0) { throw "Dynamic checks failed: $LASTEXITCODE" }
& cargo fmt --manifest-path src-tauri/Cargo.toml --check
if ($LASTEXITCODE -ne 0) { throw "Rust format check failed: $LASTEXITCODE" }
& cargo test --manifest-path src-tauri/Cargo.toml --locked
if ($LASTEXITCODE -ne 0) { throw "Rust tests failed: $LASTEXITCODE" }
& npm.cmd run build
if ($LASTEXITCODE -ne 0) { throw "Frontend build failed: $LASTEXITCODE" }
& cargo build --manifest-path src-tauri/Cargo.toml --bin CodexChatPane --features custom-protocol --release --locked
if ($LASTEXITCODE -ne 0) { throw "Rust build failed: $LASTEXITCODE" }

$output = Join-Path $projectRoot 'src-tauri/target/release/CodexChatPane.exe'
if (!(Test-Path -LiteralPath $output)) { throw "Missing release executable: $output" }
$releaseDirectory = Join-Path $projectRoot 'release'
New-Item -ItemType Directory -Path $releaseDirectory -Force | Out-Null
$releaseExe = Join-Path $releaseDirectory 'CodexChatPane.exe'
Copy-Item -LiteralPath $output -Destination $releaseExe -Force
Write-Host "[build] Ready: $releaseExe"
