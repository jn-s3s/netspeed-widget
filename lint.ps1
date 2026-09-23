# Run Ruff lint and format checks against the project venv.
# Exits non-zero when either check fails so it can gate a commit or CI step.
$ErrorActionPreference = "Stop"
$ruff = Join-Path $PSScriptRoot ".venv\Scripts\ruff.exe"

if (-not (Test-Path $ruff)) {
    Write-Host "Ruff not found at $ruff. Run: .venv\Scripts\pip install -r requirements-dev.txt" -ForegroundColor Red
    exit 1
}

& $ruff check .
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

& $ruff format --check .
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "Lint and format checks passed." -ForegroundColor Green
