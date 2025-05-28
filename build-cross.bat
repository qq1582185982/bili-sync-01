@echo off
setlocal enabledelayedexpansion

echo ========================================
echo bili-sync Cross-Platform Build Script
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

:: Install targets
echo.
echo Installing cross-compilation targets...
rustup target add x86_64-pc-windows-msvc
rustup target add x86_64-unknown-linux-musl
rustup target add aarch64-unknown-linux-musl
rustup target add x86_64-apple-darwin
rustup target add aarch64-apple-darwin

:: Check cross tool
echo Checking cross tool...
cross --version >nul 2>&1
if errorlevel 1 (
    echo Installing cross tool...
    cargo install cross --git https://github.com/cross-rs/cross
)

:: Compile Windows
echo.
echo Compiling Windows x86_64...
cargo build --release --target x86_64-pc-windows-msvc -p bili_sync
if errorlevel 0 (
    copy "target\x86_64-pc-windows-msvc\release\bili-sync-rs.exe" "%OUTPUT_DIR%\bili-sync-rs-Windows-x86_64.exe"
    echo Windows build completed
)

:: Compile Linux x86_64
echo.
echo Compiling Linux x86_64...
cross build --release --target x86_64-unknown-linux-musl -p bili_sync
if errorlevel 0 (
    copy "target\x86_64-unknown-linux-musl\release\bili-sync-rs" "%OUTPUT_DIR%\bili-sync-rs-Linux-x86_64-musl"
    echo Linux x86_64 build completed
)

:: Compile Linux ARM64
echo.
echo Compiling Linux ARM64...
cross build --release --target aarch64-unknown-linux-musl -p bili_sync
if errorlevel 0 (
    copy "target\aarch64-unknown-linux-musl\release\bili-sync-rs" "%OUTPUT_DIR%\bili-sync-rs-Linux-aarch64-musl"
    echo Linux ARM64 build completed
)

:: Compile macOS x86_64
echo.
echo Compiling macOS x86_64...
cross build --release --target x86_64-apple-darwin -p bili_sync
if errorlevel 0 (
    copy "target\x86_64-apple-darwin\release\bili-sync-rs" "%OUTPUT_DIR%\bili-sync-rs-Darwin-x86_64"
    echo macOS x86_64 build completed
)

:: Compile macOS ARM64
echo.
echo Compiling macOS ARM64...
cross build --release --target aarch64-apple-darwin -p bili_sync
if errorlevel 0 (
    copy "target\aarch64-apple-darwin\release\bili-sync-rs" "%OUTPUT_DIR%\bili-sync-rs-Darwin-aarch64"
    echo macOS ARM64 build completed
)

:: Create packages
echo.
echo Creating packages...
cd "%OUTPUT_DIR%"

:: Windows ZIP
if exist "bili-sync-rs-Windows-x86_64.exe" (
    powershell -Command "Compress-Archive -Path 'bili-sync-rs-Windows-x86_64.exe' -DestinationPath 'bili-sync-rs-Windows-x86_64.zip' -Force"
    echo Created Windows package
)

:: Linux/macOS tar.gz (if tar is available)
where tar >nul 2>&1
if not errorlevel 1 (
    if exist "bili-sync-rs-Linux-x86_64-musl" (
        tar -czf "bili-sync-rs-Linux-x86_64-musl.tar.gz" "bili-sync-rs-Linux-x86_64-musl"
        echo Created Linux x86_64 package
    )
    if exist "bili-sync-rs-Linux-aarch64-musl" (
        tar -czf "bili-sync-rs-Linux-aarch64-musl.tar.gz" "bili-sync-rs-Linux-aarch64-musl"
        echo Created Linux ARM64 package
    )
    if exist "bili-sync-rs-Darwin-x86_64" (
        tar -czf "bili-sync-rs-Darwin-x86_64.tar.gz" "bili-sync-rs-Darwin-x86_64"
        echo Created macOS x86_64 package
    )
    if exist "bili-sync-rs-Darwin-aarch64" (
        tar -czf "bili-sync-rs-Darwin-aarch64.tar.gz" "bili-sync-rs-Darwin-aarch64"
        echo Created macOS ARM64 package
    )
)

cd ..

echo.
echo ========================================
echo Build completed!
echo ========================================
echo Output directory: %OUTPUT_DIR%
dir /b "%OUTPUT_DIR%"

pause 