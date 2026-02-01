@echo off
setlocal
cd /d "%~dp0"

:: 1. Try to run the existing release binary first (Fast start)
if exist "target\release\chess_overlay.exe" (
    echo Launching Chess Overlay...
    "target\release\chess_overlay.exe"
) else (
    :: 2. Fallback: Build and run using Cargo
    echo Release binary not found. Building and running...
    cargo run --release
)

if %ERRORLEVEL% NEQ 0 (
    echo.
    echo Application exited with error code %ERRORLEVEL%.
    pause
)
