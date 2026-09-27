# Run Ruff lint/format and Pyright type-check against the project venv.
# Exits non-zero when any check fails so it can gate a commit or CI step.
$ErrorActionPreference = "Stop"
$ruff = Join-Path $PSScriptRoot ".venv\Scripts\ruff.exe"
$pyright = Join-Path $PSScriptRoot ".venv\Scripts\pyright.exe"

if (-not (Test-Path $ruff)) {
    Write-Host "Ruff not found at $ruff. Run: .venv\Scripts\pip install -r requirements-dev.txt" -ForegroundColor Red
    exit 1
}

if (-not (Test-Path $pyright)) {
    Write-Host "Pyright not found at $pyright. Run: .venv\Scripts\pip install -r requirements-dev.txt" -ForegroundColor Red
    exit 1
}

& $ruff check .
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

& $ruff format --check .
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

& $pyright
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "Lint, type-check and format checks passed." -ForegroundColor Green