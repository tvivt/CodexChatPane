$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $projectRoot
$env:CARGO_HOME = Join-Path $projectRoot '.runtime/cargo-home'
$env:CARGO_TARGET_DIR = Join-Path $projectRoot '.runtime/app-target'

Write-Host '[build] Frontend'
& npm.cmd run build
if ($LASTEXITCODE -ne 0) { throw "Frontend build failed: $LASTEXITCODE" }

Write-Host '[build] Standalone desktop app (shared dependency cache, isolated output)'
& cargo build --manifest-path src-tauri/Cargo.toml --bin codex-chat-pane --features custom-protocol --locked --offline
if ($LASTEXITCODE -ne 0) { throw "Rust build failed: $LASTEXITCODE" }

$outputDirectory = Join-Path $projectRoot '.runtime/bin'
New-Item -ItemType Directory -Path $outputDirectory -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $env:CARGO_TARGET_DIR 'debug/codex-chat-pane.exe') -Destination (Join-Path $outputDirectory 'codex-chat-pane.exe') -Force
Write-Host '[build] Ready: .runtime/bin/codex-chat-pane.exe'
