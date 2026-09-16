$ErrorActionPreference = 'Stop'
Set-Location 'C:\fast-station\code\simple-post'
npm run build 2>&1 | Out-Null
$LASTEXITCODE | Out-File -FilePath 'build-exit.txt' -Encoding utf8 -NoNewline
Get-ChildItem dist -ErrorAction SilentlyContinue | Select-Object Name, @{N='KB';E={[math]::Round($_.Length/1KB,1)}}, LastWriteTime | Format-Table -AutoSize | Out-String | Out-File 'build-output.txt' -Encoding utf8
Get-Content dist\assets\*.* -ErrorAction SilentlyContinue | Measure-Object -Sum -Property Length | ForEach-Object { '{0:N1} KB total in dist/assets' -f ($_.Sum/1KB) } | Out-File 'build-output.txt' -Append -Encoding utf8
"build complete" | Out-File 'build-output.txt' -Append -Encoding utf8
