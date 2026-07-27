#!/usr/bin/env pwsh
<#
.SYNOPSIS
    Test Bridge daemon startup and HTTP endpoints
.DESCRIPTION
    This script:
    1. Starts the bridge daemon
    2. Waits for it to be ready
    3. Tests key HTTP endpoints
    4. Stops the daemon
.NOTES
    Requires PowerShell 5.1+ and the daemon binary built
#>

# Configuration
$DAEMON_BIN = "D:\GitHub Repos\bridge-framework\target\release\daemon.exe"
$HTTP_ADDR = "127.0.0.1:18787"
$TCP_ADDR = "127.0.0.1:17878"
$REDIS_ADDR = "127.0.0.1:16399"
$BASE_URL = "http://$($HTTP_ADDR.Split(':')[0]):$($HTTP_ADDR.Split(':')[1])"

Write-Host "🚀 Bridge Daemon HTTP Startup Test" -ForegroundColor Cyan
Write-Host "===================================" -ForegroundColor Cyan
Write-Host ""

# Start daemon
Write-Host "Starting daemon on $HTTP_ADDR..." -ForegroundColor Yellow
$PROC = & $DAEMON_BIN | Out-Null -PassThru
$PID = $PROC.Id
Write-Host "✓ Daemon started (PID: $PID)" -ForegroundColor Green

# Wait for daemon to be ready
Write-Host "Waiting for daemon to be ready..." -ForegroundColor Yellow
$READY = $false
for ($i = 0; $i -lt 30; $i++) {
    try {
        $response = Invoke-WebRequest -Uri "$BASE_URL/api/v1/health" -ErrorAction SilentlyContinue
        if ($response.StatusCode -eq 200) {
            $READY = $true
            break
        }
    } catch {
        # Not ready yet, retry
    }
    Start-Sleep -Milliseconds 100
}

if (-not $READY) {
    Write-Host "✗ Daemon failed to start or is not responding" -ForegroundColor Red
    Stop-Process -Id $PID -Force -ErrorAction SilentlyContinue
    exit 1
}
Write-Host "✓ Daemon is ready" -ForegroundColor Green
Write-Host ""

# Test endpoints
Write-Host "Testing HTTP endpoints:" -ForegroundColor Yellow

$TESTS = @(
    @{ method = "GET"; path = "/api/v1/version"; name = "Version"; expect_status = 200 }
    @{ method = "GET"; path = "/api/v1/health"; name = "Health"; expect_status = 200 }
    @{ method = "GET"; path = "/api/v1/mode"; name = "Mode"; expect_status = 200 }
    @{ method = "GET"; path = "/api/v1/redis/status"; name = "Redis Status"; expect_status = 200 }
    @{ method = "GET"; path = "/api/v1/pg/status"; name = "Database Status"; expect_status = 200 }
    @{ method = "GET"; path = "/api/v1/services"; name = "Services List"; expect_status = 200 }
    @{ method = "GET"; path = "/api/v1/metrics"; name = "Metrics"; expect_status = 200 }
)

$PASSED = 0
$FAILED = 0

foreach ($TEST in $TESTS) {
    try {
        $response = Invoke-WebRequest -Uri "$BASE_URL$($TEST.path)" -Method $TEST.method -ErrorAction SilentlyContinue
        if ($response.StatusCode -eq $TEST.expect_status) {
            Write-Host "  ✓ $($TEST.name)" -ForegroundColor Green
            $PASSED++
        } else {
            Write-Host "  ✗ $($TEST.name): Expected $($TEST.expect_status), got $($response.StatusCode)" -ForegroundColor Red
            $FAILED++
        }
    } catch {
        Write-Host "  ✗ $($TEST.name): $_" -ForegroundColor Red
        $FAILED++
    }
}

Write-Host ""
Write-Host "Summary: $PASSED passed, $FAILED failed" -ForegroundColor Cyan

# Cleanup
Write-Host ""
Write-Host "Stopping daemon..." -ForegroundColor Yellow
Stop-Process -Id $PID -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 500
Write-Host "✓ Daemon stopped" -ForegroundColor Green

if ($FAILED -gt 0) {
    exit 1
}
Write-Host "✓ All tests passed!" -ForegroundColor Green
exit 0
