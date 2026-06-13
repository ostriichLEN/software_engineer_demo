$ErrorActionPreference = "Continue"

$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$records = if ($args.Count -ge 1) { $args[0] } else { "1000000" }
$sources = if ($args.Count -ge 2) { $args[1] } else { "16" }

New-Item -ItemType Directory -Force -Path target | Out-Null

Write-Host "== Build C OpenMP analytics =="
gcc c_demo/openmp_analytics.c -O2 -fopenmp -o target/c_openmp_analytics.exe
if ($LASTEXITCODE -ne 0) {
    Write-Host "OpenMP build failed. Check whether GCC supports -fopenmp."
    exit $LASTEXITCODE
}

Write-Host ""
Write-Host "== C OpenMP analytics =="
try {
    & ".\target\c_openmp_analytics.exe" $records $sources
    if ($LASTEXITCODE -ne 0) {
        Write-Host "OpenMP executable returned a non-zero exit code."
    }
} catch {
    Write-Host "OpenMP executable could not run in this environment."
    Write-Host "Common cause on Windows: Defender/antivirus false-positive on MinGW OpenMP binaries."
    Write-Host $_
}

Write-Host ""
Write-Host "== Rust Rayon analytics =="
cargo run --release -- rayon --analytics-records $records --producers $sources

