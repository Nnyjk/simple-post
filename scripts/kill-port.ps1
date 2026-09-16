$lines = netstat -ano | Select-String ':5173\s'
foreach ($line in $lines) {
  $procId = ($line -replace '\s+', ' ').Trim().Split(' ')[-1]
  if ($procId -match '^\d+$' -and $procId -ne '0') {
    Stop-Process -Id ([int]$procId) -Force -ErrorAction SilentlyContinue
    Write-Host "Killed PID $procId"
  }
}
Start-Sleep -Milliseconds 1000
$still = netstat -ano | Select-String ':5173\s'
if ($still) {
  Write-Host 'Still listening:'
  $still
} else {
  Write-Host 'Port 5173 is free'
}
