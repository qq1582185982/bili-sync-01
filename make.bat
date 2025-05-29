@echo off
setlocal enabledelayedexpansion

if "%1"=="" goto help
if "%1"=="help" goto help
if "%1"=="setup" goto setup
if "%1"=="dev" goto dev
if "%1"=="test" goto test
if "%1"=="fmt" goto fmt
if "%1"=="lint" goto lint
if "%1"=="build" goto build
if "%1"=="release" goto release
if "%1"=="clean" goto clean
if "%1"=="docs" goto docs
if "%1"=="docs-build" goto docs-build
if "%1"=="docker" goto docker
if "%1"=="compose" goto compose
if "%1"=="package" goto package

echo Unknown command: %1
goto help

:help
echo bili-sync Build Tool:
echo.
echo Development Commands:
echo   setup     - Setup development environment
echo   dev       - Start development servers
echo   test      - Run tests
echo   fmt       - Format code
echo   lint      - Lint code
echo.
echo Build Commands:
echo   build     - Build project
echo   release   - Build release version
echo   clean     - Clean build files
echo   package   - Package source code
echo.
echo Documentation Commands:
echo   docs      - Start documentation server
echo   docs-build- Build documentation
echo.
echo Docker Commands:
echo   docker    - Build Docker image
echo   compose   - Start Docker Compose
echo.
echo Usage: make.bat ^<command^>
goto end

:setup
echo Setting up development environment...
echo Checking Rust environment...
cargo --version >nul 2>&1
if errorlevel 1 (
    echo Rust not found. Please install Rust: https://rustup.rs/
    exit /b 1
)
echo Rust environment OK

echo Checking Node.js environment...
node --version >nul 2>&1
if errorlevel 1 (
    echo Node.js not found. Please install Node.js: https://nodejs.org/
    exit /b 1
)
echo Node.js environment OK

echo Installing frontend dependencies...
cd web
npm install
if errorlevel 1 (
    echo Failed to install frontend dependencies
    exit /b 1
)
echo Frontend dependencies installed

echo Building frontend...
npm run build
if errorlevel 1 (
    echo Failed to build frontend
    exit /b 1
)
cd ..
echo Frontend build complete

echo Installing Rust dependencies...
cargo check
if errorlevel 1 (
    echo Failed to install Rust dependencies
    exit /b 1
)
echo Rust dependencies installed

echo Installing documentation dependencies...
cd docs
npm install
if errorlevel 1 (
    echo Failed to install documentation dependencies
    exit /b 1
)
cd ..
echo Documentation dependencies installed

echo Development environment setup complete!
goto end

:dev
echo Starting development servers...
echo Starting Rust backend...
start "Rust Backend" cmd /k "cargo run --bin bili-sync-rs"
timeout /t 2 /nobreak >nul
echo Starting Svelte frontend...
start "Svelte Frontend" cmd /k "cd web && npm run dev"
echo All services started!
echo Backend API: http://localhost:12345
echo Frontend UI: http://localhost:5173
goto end

:test
echo Running tests...
cargo test
if errorlevel 1 (
    echo Tests failed
    exit /b 1
) else (
    echo All tests passed
)
goto end

:fmt
echo Formatting code...
cargo fmt
echo Code formatting complete
goto end

:lint
echo Linting code...
cargo clippy -- -D warnings
goto end

:build
echo Building project...
echo [DEBUG] Starting frontend build...
cd web
if not exist "node_modules" (
    echo Installing frontend dependencies...
    call npm install
    if errorlevel 1 (
        echo Failed to install frontend dependencies
        exit /b 1
    )
)
echo [DEBUG] Running npm run build...
call npm run build
if errorlevel 1 (
    echo Failed to build frontend
    exit /b 1
)
echo [DEBUG] Frontend build completed, returning to root directory...
cd ..
echo [DEBUG] Starting Rust backend build...
cargo build
if errorlevel 1 (
    echo Failed to build backend
    exit /b 1
)
echo [DEBUG] Backend build completed
echo Project build complete
goto end

:release
echo Building release version...
cd web
if not exist "node_modules" (
    echo Installing frontend dependencies...
    npm install
    if errorlevel 1 (
        echo Failed to install frontend dependencies
        exit /b 1
    )
)
npm run build
if errorlevel 1 (
    echo Failed to build frontend
    exit /b 1
)
cd ..
cargo build --release
if errorlevel 1 (
    echo Failed to build backend
    exit /b 1
)
echo Release build complete
goto end

:clean
echo Cleaning build files...
cargo clean
if exist "web\build" rmdir /s /q "web\build"
if exist "web\.svelte-kit" rmdir /s /q "web\.svelte-kit"
if exist "web\node_modules" rmdir /s /q "web\node_modules"
if exist "docs\.vitepress\dist" rmdir /s /q "docs\.vitepress\dist"
if exist "docs\node_modules" rmdir /s /q "docs\node_modules"
echo Clean complete
goto end

:package
echo Packaging source code...
echo Step 1: Cleaning build files...
call :clean

echo Step 2: Creating source package...
:: Get current date and time in YYYY-MM-DD_HH-MM-SS format using PowerShell
for /f %%i in ('powershell -Command "Get-Date -Format 'yyyy-MM-dd_HH-mm-ss'"') do set timestamp=%%i
set packageName=bili-sync-source-%timestamp%.zip

echo Package name: %packageName%

:: Create temp directory
set tempDir=temp_package
if exist "%tempDir%" rmdir /s /q "%tempDir%"
mkdir "%tempDir%"

:: Copy files
echo Including: .github
if exist ".github" (
    xcopy /s /e /q ".github" "%tempDir%\.github\" >nul 2>&1
    if errorlevel 1 echo WARNING: Failed to copy .github
) else (
    echo WARNING: .github folder not found
)

echo Including: crates
if exist "crates" (
    xcopy /s /e /q "crates" "%tempDir%\crates\" >nul 2>&1
    if errorlevel 1 echo WARNING: Failed to copy crates
) else (
    echo WARNING: crates folder not found
)

echo Including: web
if exist "web" (
    xcopy /s /e /q "web" "%tempDir%\web\" >nul 2>&1
    if errorlevel 1 echo WARNING: Failed to copy web
) else (
    echo WARNING: web folder not found
)

echo Including: docs
if exist "docs" (
    xcopy /s /e /q "docs" "%tempDir%\docs\" >nul 2>&1
    if errorlevel 1 echo WARNING: Failed to copy docs
) else (
    echo WARNING: docs folder not found
)

echo Including: scripts
if exist "scripts" (
    xcopy /s /e /q "scripts" "%tempDir%\scripts\" >nul 2>&1
    if errorlevel 1 echo WARNING: Failed to copy scripts
) else (
    echo WARNING: scripts folder not found
)

echo Including: assets
if exist "assets" (
    xcopy /s /e /q "assets" "%tempDir%\assets\" >nul 2>&1
    if errorlevel 1 echo WARNING: Failed to copy assets
) else (
    echo WARNING: assets folder not found
)

echo Including: Cargo.toml
copy "Cargo.toml" "%tempDir%\" >nul
echo Including: Cargo.lock
copy "Cargo.lock" "%tempDir%\" >nul
echo Including: Dockerfile
copy "Dockerfile" "%tempDir%\" >nul
echo Including: docker-compose.yml
copy "docker-compose.yml" "%tempDir%\" >nul
echo Including: README.md
copy "README.md" "%tempDir%\" >nul
echo Including: rustfmt.toml
copy "rustfmt.toml" "%tempDir%\" >nul
echo Including: .gitignore
copy ".gitignore" "%tempDir%\" >nul
echo Including: .dockerignore
copy ".dockerignore" "%tempDir%\" >nul
echo Including: config.toml
copy "config.toml" "%tempDir%\" >nul
echo Including: make.bat
copy "make.bat" "%tempDir%\" >nul
echo Including: setup-github-actions.bat
copy "setup-github-actions.bat" "%tempDir%\" >nul
echo Including: copy-to-git.bat
copy "copy-to-git.bat" "%tempDir%\" >nul
echo Including: files-to-upload.txt
copy "files-to-upload.txt" "%tempDir%\" >nul
echo Including: install-docker-wsl.sh
copy "install-docker-wsl.sh" "%tempDir%\" >nul

:: Clean up unwanted items in temp directory
if exist "%tempDir%\web\node_modules" rmdir /s /q "%tempDir%\web\node_modules"
if exist "%tempDir%\web\build" rmdir /s /q "%tempDir%\web\build"
if exist "%tempDir%\web\.svelte-kit" rmdir /s /q "%tempDir%\web\.svelte-kit"
if exist "%tempDir%\docs\node_modules" rmdir /s /q "%tempDir%\docs\node_modules"
if exist "%tempDir%\docs\.vitepress\dist" rmdir /s /q "%tempDir%\docs\.vitepress\dist"

:: Create ZIP using PowerShell
echo Creating ZIP package...
powershell -Command "Compress-Archive -Path '%tempDir%\*' -DestinationPath '%packageName%' -Force"

if exist "%packageName%" (
    echo Package created successfully!
    for %%A in ("%packageName%") do (
        set /a sizeInMB=%%~zA/1024/1024
        echo File: %%~nxA
        echo Size: !sizeInMB! MB
    )
) else (
    echo Failed to create package
    exit /b 1
)

:: Cleanup
rmdir /s /q "%tempDir%"
echo Packaging complete!
goto end

:docs
echo Starting documentation server...
cd docs
if not exist "node_modules" (
    echo Installing documentation dependencies...
    call npm install
    if errorlevel 1 (
        echo Failed to install documentation dependencies
        exit /b 1
    )
)
npm run docs:dev
cd ..
goto end

:docs-build
echo Building documentation...
cd docs
if not exist "node_modules" (
    echo Installing documentation dependencies...
    call npm install
    if errorlevel 1 (
        echo Failed to install documentation dependencies
        exit /b 1
    )
)
npm run docs:build
cd ..
echo Documentation build complete
goto end

:docker
echo Building Docker image...
docker build -t bili-sync .
goto end

:compose
echo Starting Docker Compose...
docker-compose up -d
goto end

:end 