$ErrorActionPreference = "Continue"

function Invoke-ExpectedFailure {
    param(
        [string]$Label,
        [string]$Command,
        [string[]]$Arguments
    )

    Write-Host ""
    Write-Host "== $Label =="
    & $Command @Arguments
    $code = $LASTEXITCODE

    if ($code -eq 0) {
        Write-Host "UNEXPECTED: command succeeded, but this demo should fail."
        exit 1
    }

    Write-Host "OK: compiler rejected this unsafe pattern as expected."
}

function Invoke-ExpectedSuccess {
    param(
        [string]$Label,
        [string]$Command,
        [string[]]$Arguments
    )

    Write-Host ""
    Write-Host "== $Label =="
    & $Command @Arguments
    $code = $LASTEXITCODE

    if ($code -ne 0) {
        Write-Host "UNEXPECTED: command failed, but this demo should pass."
        exit $code
    }

    Write-Host "OK: safe version compiled and ran successfully."
}

$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

Invoke-ExpectedFailure `
    -Label "mpsc ownership transfer: use after send" `
    -Command "rustc" `
    -Arguments @("safety_demos/mpsc_use_after_send.rs")

Invoke-ExpectedFailure `
    -Label "mpsc Send bound: Rc cannot cross threads" `
    -Command "rustc" `
    -Arguments @("safety_demos/mpsc_non_send_rc.rs")

Invoke-ExpectedFailure `
    -Label "Rayon shared mutable Vec is rejected" `
    -Command "cargo" `
    -Arguments @("check", "--offline", "--manifest-path", "safety_demos/rayon_shared_mutation/Cargo.toml")

Invoke-ExpectedSuccess `
    -Label "Rayon fold/reduce safe aggregation" `
    -Command "cargo" `
    -Arguments @("run", "--release", "--offline", "--manifest-path", "safety_demos/rayon_safe_fold/Cargo.toml")

Write-Host ""
Write-Host "All safety demos behaved as expected."
