@echo off
echo ========================================
echo GitHub Actions Setup Script
echo ========================================

echo This script will help you set up GitHub Actions for cross-platform builds.
echo.

:: Check if git is available
git --version >nul 2>&1
if errorlevel 1 (
    echo Error: Git not found. Please install Git first.
    pause
    exit /b 1
)

echo Git is available.
echo.

:: Check if this is already a git repository
if not exist ".git" (
    echo Initializing Git repository...
    git init
    echo Git repository initialized.
) else (
    echo Git repository already exists.
)

:: Add files to git
echo Adding files to Git...
git add .github/workflows/
git add docs/github-actions-build.md
git add build-windows.bat
git add build-cross.bat

:: Check git status
echo.
echo Current Git status:
git status --short

echo.
echo ========================================
echo Setup Complete!
echo ========================================
echo.
echo Next steps:
echo 1. Commit the changes:
echo    git commit -m "Add GitHub Actions workflows"
echo.
echo 2. Push to GitHub:
echo    git remote add origin https://github.com/YOUR_USERNAME/YOUR_REPO.git
echo    git push -u origin main
echo.
echo 3. Go to your GitHub repository
echo 4. Click on "Actions" tab
echo 5. Select "Manual Build" workflow
echo 6. Click "Run workflow" to start building
echo.
echo For detailed instructions, see: docs/github-actions-build.md
echo.

pause 