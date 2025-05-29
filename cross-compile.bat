@echo off
chcp 65001 >nul
setlocal enabledelayedexpansion

echo ========================================
echo bili-sync Cross-Platform Build Script
echo ========================================

:: Check Rust environment
echo Checking Rust environment...
cargo --version >nul 2>&1
if errorlevel 1 (
    echo Error: Rust not found. Please install Rust: https://rustup.rs/
    exit /b 1
)
echo Rust environment check passed

:: Create output directory
set OUTPUT_DIR=releases
if exist "%OUTPUT_DIR%" rmdir /s /q "%OUTPUT_DIR%"
mkdir "%OUTPUT_DIR%"

:: Get version number (fix workspace issue)
echo Getting project version...
for /f "tokens=*" %%i in ('cargo pkgid -p bili_sync 2^>nul ^| findstr /r "bili_sync#.*"') do set PACKAGE_INFO=%%i
if "%PACKAGE_INFO%"=="" (
    echo Warning: Cannot get version number, using default version
    set VERSION=2.5.1
) else (
    for /f "tokens=2 delims=#" %%i in ("%PACKAGE_INFO%") do set VERSION=%%i
)
if "%VERSION%"=="" set VERSION=2.5.1

echo Current version: %VERSION%

:: Define target platforms
set TARGETS[0]=x86_64-pc-windows-msvc
set TARGETS[1]=x86_64-unknown-linux-musl
set TARGETS[2]=aarch64-unknown-linux-musl
set TARGETS[3]=x86_64-apple-darwin
set TARGETS[4]=aarch64-apple-darwin

set TARGET_NAMES[0]=Windows-x86_64
set TARGET_NAMES[1]=Linux-x86_64-musl
set TARGET_NAMES[2]=Linux-aarch64-musl
set TARGET_NAMES[3]=Darwin-x86_64
set TARGET_NAMES[4]=Darwin-aarch64

:: First build frontend
echo.
echo ========================================
echo Building frontend resources...
echo ========================================
if exist "web" (
    cd web
    if not exist "node_modules" (
        echo Installing frontend dependencies...
        call npm install
        if errorlevel 1 (
            echo Error: Frontend dependency installation failed
            cd ..
            exit /b 1
        )
    )
    echo Building frontend...
    call npm run build
    if errorlevel 1 (
        echo Error: Frontend build failed
        cd ..
        exit /b 1
    )
    cd ..
    echo Frontend build completed
) else (
    echo Warning: web directory not found, skipping frontend build
)

:: Install cross-compilation targets
echo.
echo ========================================
echo Installing cross-compilation targets...
echo ========================================

for /l %%i in (0,1,4) do (
    echo Installing target: !TARGETS[%%i]!
    rustup target add !TARGETS[%%i]! >nul 2>&1
)

:: Check if cross tool needs to be installed
echo Checking cross tool...
cross --version >nul 2>&1
if errorlevel 1 (
    echo Installing cross tool...
    cargo install cross --git https://github.com/cross-rs/cross
    if errorlevel 1 (
        echo Warning: cross installation failed, will use cargo for compilation
        set "USE_CROSS=false"
    ) else (
        set "USE_CROSS=true"
    )
) else (
    set "USE_CROSS=true"
    echo cross tool already installed
)

:: Start compilation
echo.
echo ========================================
echo Starting cross-platform compilation...
echo ========================================

for /l %%i in (0,1,4) do (
    set "TARGET=!TARGETS[%%i]!"
    set "TARGET_NAME=!TARGET_NAMES[%%i]!"
    
    echo.
    echo Compiling target: !TARGET_NAME! (!TARGET!)
    echo ----------------------------------------
    
    :: Choose compilation method based on platform
    if "!TARGET!"=="x86_64-pc-windows-msvc" (
        :: Use cargo for Windows platform, specify package name
        echo Using cargo to compile Windows version...
        cargo build --release --target !TARGET! -p bili_sync
        set "BUILD_SUCCESS=!errorlevel!"
        set "BINARY_NAME=bili-sync-rs.exe"
    ) else (
        if "!USE_CROSS!"=="true" (
            :: Use cross for other platforms, specify package name
            echo Using cross to compile !TARGET_NAME! version...
            cross build --release --target !TARGET! -p bili_sync
            set "BUILD_SUCCESS=!errorlevel!"
            set "BINARY_NAME=bili-sync-rs"
        ) else (
            :: Fallback to cargo, specify package name
            echo Using cargo to compile !TARGET_NAME! version...
            cargo build --release --target !TARGET! -p bili_sync
            set "BUILD_SUCCESS=!errorlevel!"
            set "BINARY_NAME=bili-sync-rs"
        )
    )
    
    if "!BUILD_SUCCESS!"=="0" (
        echo ✓ Compilation successful: !TARGET_NAME!
        
        :: Copy and rename binary file
        set "SOURCE_PATH=target\!TARGET!\release\!BINARY_NAME!"
        if "!TARGET!"=="x86_64-pc-windows-msvc" (
            set "OUTPUT_NAME=bili-sync-rs-!TARGET_NAME!.exe"
        ) else (
            set "OUTPUT_NAME=bili-sync-rs-!TARGET_NAME!"
        )
        
        if exist "!SOURCE_PATH!" (
            copy "!SOURCE_PATH!" "%OUTPUT_DIR%\!OUTPUT_NAME!" >nul
            echo   → %OUTPUT_DIR%\!OUTPUT_NAME!
            
            :: Display file size
            for %%A in ("%OUTPUT_DIR%\!OUTPUT_NAME!") do (
                set /a sizeInMB=%%~zA/1024/1024
                echo   → Size: !sizeInMB! MB
            )
        ) else (
            echo ✗ Error: Cannot find compilation output file !SOURCE_PATH!
        )
    ) else (
        echo ✗ Compilation failed: !TARGET_NAME!
    )
)

:: Create compressed packages
echo.
echo ========================================
echo Creating release packages...
echo ========================================

for /l %%i in (0,1,4) do (
    set "TARGET_NAME=!TARGET_NAMES[%%i]!"
    
    if "!TARGET_NAME!"=="Windows-x86_64" (
        set "BINARY_FILE=bili-sync-rs-!TARGET_NAME!.exe"
        set "ARCHIVE_NAME=bili-sync-rs-!TARGET_NAME!.zip"
    ) else (
        set "BINARY_FILE=bili-sync-rs-!TARGET_NAME!"
        set "ARCHIVE_NAME=bili-sync-rs-!TARGET_NAME!.tar.gz"
    )
    
    if exist "%OUTPUT_DIR%\!BINARY_FILE!" (
        echo Creating package: !ARCHIVE_NAME!
        
        if "!TARGET_NAME!"=="Windows-x86_64" (
            :: Use ZIP for Windows
            powershell -Command "Compress-Archive -Path '%OUTPUT_DIR%\!BINARY_FILE!' -DestinationPath '%OUTPUT_DIR%\!ARCHIVE_NAME!' -Force"
        ) else (
            :: Use tar.gz for Linux/macOS (requires WSL or Git Bash)
            where tar >nul 2>&1
            if !errorlevel! equ 0 (
                cd "%OUTPUT_DIR%"
                tar -czf "!ARCHIVE_NAME!" "!BINARY_FILE!"
                cd ..
            ) else (
                echo Warning: tar command not found, skipping !ARCHIVE_NAME! creation
            )
        )
        
        if exist "%OUTPUT_DIR%\!ARCHIVE_NAME!" (
            for %%A in ("%OUTPUT_DIR%\!ARCHIVE_NAME!") do (
                set /a sizeInMB=%%~zA/1024/1024
                echo   ✓ !ARCHIVE_NAME! (!sizeInMB! MB)
            )
        )
    )
)

echo.
echo ========================================
echo Compilation completed!
echo ========================================
echo Output directory: %OUTPUT_DIR%
echo.
echo Generated files:
dir /b "%OUTPUT_DIR%"

echo.
echo Usage:
echo   1. Extract the corresponding platform package
echo   2. Run the bili-sync-rs executable
echo   3. Use --help to view help information

pause 