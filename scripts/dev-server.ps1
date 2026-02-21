param(
  [Parameter(Position = 0)]
  [ValidateSet("start", "stop", "restart", "status")]
  [string]$Action = "status",

  [switch]$Foreground
)

$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = (Resolve-Path (Join-Path $scriptDir "..")).Path
$appDir = Join-Path $repoRoot "dictum-app"
$stateDir = Join-Path $repoRoot ".dev"
$pidFile = Join-Path $stateDir "tauri-dev.pid"
$logFile = Join-Path $stateDir "tauri-dev.log"

function Ensure-StateDir {
  if (-not (Test-Path -LiteralPath $stateDir)) {
    New-Item -ItemType Directory -Path $stateDir | Out-Null
  }
}

function Read-Pid {
  if (-not (Test-Path -LiteralPath $pidFile)) {
    return $null
  }
  $raw = (Get-Content -LiteralPath $pidFile -ErrorAction SilentlyContinue | Select-Object -First 1)
  if ($raw -match "^\d+$") {
    return [int]$raw
  }
  return $null
}

function Remove-PidFile {
  if (Test-Path -LiteralPath $pidFile) {
    Remove-Item -LiteralPath $pidFile -Force -ErrorAction SilentlyContinue
  }
}

function Is-Running([int]$ProcessId) {
  try {
    $null = Get-Process -Id $ProcessId -ErrorAction Stop
    return $true
  } catch {
    return $false
  }
}

function Start-DevServer {
  Ensure-StateDir

  $existingPid = Read-Pid
  if ($existingPid -and (Is-Running $existingPid)) {
    Write-Host "Dev server already running (PID $existingPid)."
    Write-Host "Log: $logFile"
    return
  }
  if ($existingPid) {
    Remove-PidFile
  }

  if ($Foreground) {
    Push-Location $appDir
    try {
      cargo tauri dev
    } finally {
      Pop-Location
    }
    return
  }

  $escapedAppDir = $appDir.Replace("'", "''")
  $escapedLogFile = $logFile.Replace("'", "''")
  $cmd = "Set-Location -LiteralPath '$escapedAppDir'; cargo tauri dev *>> '$escapedLogFile'"

  $proc = Start-Process `
    -FilePath "powershell.exe" `
    -ArgumentList "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", $cmd `
    -WindowStyle Hidden `
    -PassThru

  Set-Content -LiteralPath $pidFile -Value $proc.Id -Encoding ascii
  Write-Host "Started dev server (PID $($proc.Id))."
  Write-Host "Log: $logFile"
}

function Stop-DevServer {
  $serverPid = Read-Pid
  if (-not $serverPid) {
    Write-Host "No running dev server (no PID file)."
    return
  }

  if (-not (Is-Running $serverPid)) {
    Write-Host "Dev server PID file exists but process $serverPid is not running."
    Remove-PidFile
    return
  }

  cmd /c "taskkill /PID $serverPid /T /F" | Out-Null
  Start-Sleep -Milliseconds 250

  if (Is-Running $serverPid) {
    Stop-Process -Id $serverPid -Force -ErrorAction SilentlyContinue
  }

  Remove-PidFile
  Write-Host "Stopped dev server (PID $serverPid)."
}

function Show-Status {
  $serverPid = Read-Pid
  if (-not $serverPid) {
    Write-Host "Dev server status: stopped"
    return
  }
  if (Is-Running $serverPid) {
    Write-Host "Dev server status: running (PID $serverPid)"
    Write-Host "Log: $logFile"
  } else {
    Write-Host "Dev server status: stopped (stale PID file found: $serverPid)"
  }
}

switch ($Action) {
  "start" { Start-DevServer }
  "stop" { Stop-DevServer }
  "restart" {
    Stop-DevServer
    Start-DevServer
  }
  "status" { Show-Status }
}
