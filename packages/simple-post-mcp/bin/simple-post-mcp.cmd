@echo off
REM packages/simple-post-mcp/bin/simple-post-mcp.cmd
REM
REM npm wrapper entry point (Windows). Resolves the native binary that
REM ships inside the npm package and forwards every argument. Mirrors
REM the bash script in `bin/simple-post-mcp`; both are kept in sync.

setlocal
set "SCRIPT_DIR=%~dp0"
set "PACKAGE_ROOT=%SCRIPT_DIR%.."
set "NATIVE_DIR=%PACKAGE_ROOT%\native\current"
set "NATIVE_BIN=%NATIVE_DIR%\simple-post-mcp.exe"

if not exist "%NATIVE_BIN%" (
  echo [simple-post-mcp] native binary missing at "%NATIVE_BIN%" 1>&2
  echo [simple-post-mcp] try: npm install --force @simple-post/mcp-server 1>&2
  exit /b 127
)

"%NATIVE_BIN%" %*
endlocal