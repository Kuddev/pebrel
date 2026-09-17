@echo off
setlocal
cd /d "%~dp0"
set "PEBREL_CONFIG_DIR=%~dp0preview-profile"
set "PEBREL_RUNTIME_ENDPOINT="
set "NEBULA_RUNTIME_ENDPOINT="
start "" "%~dp0pebrel.exe"
endlocal
