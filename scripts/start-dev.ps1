param([switch]$FrontendOnly)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
if (-not (Get-Command pnpm -ErrorAction SilentlyContinue)) {
    throw 'Install Node.js and pnpm, then add pnpm to PATH.'
}
if (-not $FrontendOnly -and -not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw 'Install Rust and the Tauri 2 system prerequisites, then add cargo to PATH.'
}
Push-Location -LiteralPath $projectRoot
try {
    if (-not (Test-Path -LiteralPath 'node_modules')) {
        & pnpm install --frozen-lockfile
        if ($LASTEXITCODE -ne 0) { throw 'Dependency installation failed.' }
    }
    if ($FrontendOnly) { & pnpm dev }
    else { & pnpm tauri dev }
    if ($LASTEXITCODE -ne 0) { throw 'Development process exited with an error.' }
} finally { Pop-Location }
