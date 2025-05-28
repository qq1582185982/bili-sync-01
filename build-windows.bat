@echo off
setlocal enabledelayedexpansion

echo ========================================
echo bili-sync Windows Build Script
echo ========================================

:: Check Rust environment
echo Checking Rust environment...
cargo --version >nul 2>&1
if errorlevel 1 (
    echo Error: Rust not found. Please install Rust
    exit /b 1
)
echo Rust environment OK

:: Create output directory
set OUTPUT_DIR=releases
if exist "%OUTPUT_DIR%" rmdir /s /q "%OUTPUT_DIR%"
mkdir "%OUTPUT_DIR%"

:: Get version
echo Getting project version...
set VERSION=2.5.1
echo Current version: %VERSION%

:: Build frontend first
echo.
echo Building frontend...
if exist "web" (
    cd web
    call npm run build
    if errorlevel 1 (
        echo Frontend build failed
        cd ..
        exit /b 1
    )
    cd ..
    echo Frontend build completed
)

:: Install Windows target
echo.
echo Installing Windows target...
rustup target add x86_64-pc-windows-msvc

:: Compile Windows
echo.
echo Compiling Windows x86_64...
cargo build --release --target x86_64-pc-windows-msvc -p bili_sync
if errorlevel 0 (
    copy "target\x86_64-pc-windows-msvc\release\bili-sync-rs.exe" "%OUTPUT_DIR%\bili-sync-rs-Windows-x86_64.exe"
    echo Windows build completed
    
    :: Show file size
    for %%A in ("%OUTPUT_DIR%\bili-sync-rs-Windows-x86_64.exe") do (
        set /a sizeInMB=%%~zA/1024/1024
        echo Binary size: !sizeInMB! MB
    )
) else (
    echo Windows build failed
    exit /b 1
)

:: Create ZIP package
echo.
echo Creating Windows package...
cd "%OUTPUT_DIR%"
if exist "bili-sync-rs-Windows-x86_64.exe" (
    powershell -Command "Compress-Archive -Path 'bili-sync-rs-Windows-x86_64.exe' -DestinationPath 'bili-sync-rs-Windows-x86_64.zip' -Force"
    echo Created Windows package
    
    :: Show package size
    for %%A in ("bili-sync-rs-Windows-x86_64.zip") do (
        set /a sizeInMB=%%~zA/1024/1024
        echo Package size: !sizeInMB! MB
    )
)
cd ..

echo.
echo ========================================
echo Windows Build Completed!
echo ========================================
echo Output directory: %OUTPUT_DIR%
echo.
echo Generated files:
dir /b "%OUTPUT_DIR%"

echo.
echo Usage:
echo   1. Extract bili-sync-rs-Windows-x86_64.zip
echo   2. Run bili-sync-rs-Windows-x86_64.exe
echo   3. Use --help to view help information

pause 