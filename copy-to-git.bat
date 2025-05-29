@echo off
setlocal enabledelayedexpansion

echo ========================================
echo Copy Files to Git Repository
echo ========================================

set /p GIT_DIR="请输入你的 Git 仓库目录路径: "

if not exist "%GIT_DIR%" (
    echo 错误: 目录 "%GIT_DIR%" 不存在
    pause
    exit /b 1
)

echo.
echo 开始复制文件到: %GIT_DIR%
echo.

:: 创建必要的目录
echo 创建目录结构...
if not exist "%GIT_DIR%\.github\workflows" mkdir "%GIT_DIR%\.github\workflows"
if not exist "%GIT_DIR%\docs" mkdir "%GIT_DIR%\docs"

:: 复制 GitHub Actions 工作流文件
echo 复制 GitHub Actions 工作流...
if exist ".github\workflows\build.yml" (
    copy ".github\workflows\build.yml" "%GIT_DIR%\.github\workflows\"
    echo   ✓ build.yml
) else (
    echo   ✗ build.yml 不存在
)

if exist ".github\workflows\manual-build.yml" (
    copy ".github\workflows\manual-build.yml" "%GIT_DIR%\.github\workflows\"
    echo   ✓ manual-build.yml
) else (
    echo   ✗ manual-build.yml 不存在
)

:: 复制文档文件
echo 复制文档文件...
if exist "docs\github-actions-build.md" (
    copy "docs\github-actions-build.md" "%GIT_DIR%\docs\"
    echo   ✓ github-actions-build.md
) else (
    echo   ✗ github-actions-build.md 不存在
)

:: 复制编译脚本
echo 复制编译脚本...
if exist "build-windows.bat" (
    copy "build-windows.bat" "%GIT_DIR%\"
    echo   ✓ build-windows.bat
) else (
    echo   ✗ build-windows.bat 不存在
)

if exist "build-cross.bat" (
    copy "build-cross.bat" "%GIT_DIR%\"
    echo   ✓ build-cross.bat
) else (
    echo   ✗ build-cross.bat 不存在
)

if exist "setup-github-actions.bat" (
    copy "setup-github-actions.bat" "%GIT_DIR%\"
    echo   ✓ setup-github-actions.bat
) else (
    echo   ✗ setup-github-actions.bat 不存在
)

:: 复制项目核心文件
echo 复制项目核心文件...
if exist "Cargo.toml" (
    copy "Cargo.toml" "%GIT_DIR%\"
    echo   ✓ Cargo.toml
)

if exist "Cargo.lock" (
    copy "Cargo.lock" "%GIT_DIR%\"
    echo   ✓ Cargo.lock
)

if exist ".gitignore" (
    copy ".gitignore" "%GIT_DIR%\"
    echo   ✓ .gitignore
)

if exist "README.md" (
    copy "README.md" "%GIT_DIR%\"
    echo   ✓ README.md
)

:: 复制源代码目录
echo 复制源代码目录...
if exist "crates" (
    xcopy "crates" "%GIT_DIR%\crates" /E /I /Y >nul
    echo   ✓ crates/ 目录
)

if exist "web" (
    xcopy "web" "%GIT_DIR%\web" /E /I /Y >nul
    echo   ✓ web/ 目录
)

echo.
echo ========================================
echo 复制完成！
echo ========================================
echo.
echo 已复制的文件:
echo   📁 .github/workflows/
echo   📁 docs/
echo   📁 crates/
echo   📁 web/
echo   📄 *.bat 脚本文件
echo   📄 Cargo.toml, Cargo.lock
echo   📄 README.md, .gitignore
echo.
echo 下一步操作:
echo 1. 进入你的 Git 目录: cd "%GIT_DIR%"
echo 2. 添加文件: git add .
echo 3. 提交: git commit -m "Add GitHub Actions and build scripts"
echo 4. 推送: git push origin main
echo.

pause 