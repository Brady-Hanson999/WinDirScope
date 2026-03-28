@echo off
setlocal

call "C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools\Common7\Tools\VsDevCmd.bat" -arch=x64 -host_arch=x64
if errorlevel 1 exit /b %errorlevel%

set "CARGO_TARGET_DIR=%LOCALAPPDATA%\WinDirScope\target"
if not exist "%CARGO_TARGET_DIR%" mkdir "%CARGO_TARGET_DIR%"
if errorlevel 1 exit /b %errorlevel%

npx %*
