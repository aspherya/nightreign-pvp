@echo off
setlocal DisableDelayedExpansion
pushd "%~dp0"
if errorlevel 1 exit /b 1
set "build_powershell=%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe"
"%SystemRoot%\System32\where.exe" pwsh.exe >nul 2>&1
if not errorlevel 1 set "build_powershell=pwsh.exe"
"%build_powershell%" -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\build-local.ps1" -Task Launcher %*
set "build_exit=%ERRORLEVEL%"
if not "%NRPVP_NO_PAUSE%"=="1" pause
popd
exit /b %build_exit%
