# bili-sync 跨平台编译脚本 (PowerShell 版本)
param(
    [switch]$SkipFrontend,
    [switch]$SkipCompress,
    [string]$OutputDir = "releases"
)

Write-Host "========================================" -ForegroundColor Cyan
Write-Host "bili-sync 跨平台编译脚本" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan

# 检查 Rust 环境
Write-Host "检查 Rust 环境..." -ForegroundColor Yellow
try {
    $rustVersion = cargo --version
    Write-Host "✓ Rust 环境检查通过: $rustVersion" -ForegroundColor Green
} catch {
    Write-Host "✗ 错误: 未找到 Rust。请安装 Rust: https://rustup.rs/" -ForegroundColor Red
    exit 1
}

# 创建输出目录
if (Test-Path $OutputDir) {
    Remove-Item $OutputDir -Recurse -Force
}
New-Item -ItemType Directory -Path $OutputDir | Out-Null

# 获取版本号
try {
    $packageInfo = cargo pkgid | Select-String "bili_sync#"
    $version = ($packageInfo -split "#")[1]
    if (-not $version) { $version = "unknown" }
} catch {
    $version = "unknown"
}

Write-Host "当前版本: $version" -ForegroundColor Green

# 定义目标平台
$targets = @{
    "x86_64-pc-windows-msvc" = "Windows-x86_64"
    "x86_64-unknown-linux-musl" = "Linux-x86_64-musl"
    "aarch64-unknown-linux-musl" = "Linux-aarch64-musl"
    "x86_64-apple-darwin" = "Darwin-x86_64"
    "aarch64-apple-darwin" = "Darwin-aarch64"
}

# 构建前端
if (-not $SkipFrontend) {
    Write-Host ""
    Write-Host "========================================" -ForegroundColor Cyan
    Write-Host "构建前端资源..." -ForegroundColor Cyan
    Write-Host "========================================" -ForegroundColor Cyan
    
    Push-Location web
    
    if (-not (Test-Path "node_modules")) {
        Write-Host "安装前端依赖..." -ForegroundColor Yellow
        npm install
        if ($LASTEXITCODE -ne 0) {
            Write-Host "✗ 错误: 前端依赖安装失败" -ForegroundColor Red
            Pop-Location
            exit 1
        }
    }
    
    npm run build
    if ($LASTEXITCODE -ne 0) {
        Write-Host "✗ 错误: 前端构建失败" -ForegroundColor Red
        Pop-Location
        exit 1
    }
    
    Pop-Location
    Write-Host "✓ 前端构建完成" -ForegroundColor Green
}

# 安装交叉编译目标
Write-Host ""
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "安装交叉编译目标..." -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan

foreach ($target in $targets.Keys) {
    Write-Host "安装目标: $target" -ForegroundColor Yellow
    rustup target add $target | Out-Null
}

# 检查 cross 工具
Write-Host "检查 cross 工具..." -ForegroundColor Yellow
$useCross = $true
try {
    cross --version | Out-Null
    Write-Host "✓ cross 工具已安装" -ForegroundColor Green
} catch {
    Write-Host "安装 cross 工具..." -ForegroundColor Yellow
    cargo install cross --git https://github.com/cross-rs/cross
    if ($LASTEXITCODE -ne 0) {
        Write-Host "⚠ 警告: cross 安装失败，将使用 cargo 进行编译" -ForegroundColor Yellow
        $useCross = $false
    } else {
        Write-Host "✓ cross 工具安装完成" -ForegroundColor Green
    }
}

# 开始编译
Write-Host ""
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "开始跨平台编译..." -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan

$successCount = 0
$totalCount = $targets.Count

foreach ($target in $targets.Keys) {
    $targetName = $targets[$target]
    
    Write-Host ""
    Write-Host "编译目标: $targetName ($target)" -ForegroundColor Yellow
    Write-Host "----------------------------------------" -ForegroundColor Gray
    
    # 根据平台选择编译方式
    $buildSuccess = $false
    if ($target -eq "x86_64-pc-windows-msvc") {
        # Windows 平台使用 cargo
        cargo build --release --target $target
        $buildSuccess = ($LASTEXITCODE -eq 0)
        $binaryName = "bili-sync-rs.exe"
    } elseif ($useCross) {
        # 其他平台使用 cross
        cross build --release --target $target
        $buildSuccess = ($LASTEXITCODE -eq 0)
        $binaryName = "bili-sync-rs"
    } else {
        # 回退到 cargo
        cargo build --release --target $target
        $buildSuccess = ($LASTEXITCODE -eq 0)
        $binaryName = "bili-sync-rs"
    }
    
    if ($buildSuccess) {
        Write-Host "✓ 编译成功: $targetName" -ForegroundColor Green
        $successCount++
        
        # 复制并重命名二进制文件
        $sourcePath = "target\$target\release\$binaryName"
        if ($target -eq "x86_64-pc-windows-msvc") {
            $outputName = "bili-sync-rs-$targetName.exe"
        } else {
            $outputName = "bili-sync-rs-$targetName"
        }
        
        if (Test-Path $sourcePath) {
            Copy-Item $sourcePath "$OutputDir\$outputName"
            Write-Host "  → $OutputDir\$outputName" -ForegroundColor Gray
            
            # 显示文件大小
            $fileSize = (Get-Item "$OutputDir\$outputName").Length
            $sizeInMB = [math]::Round($fileSize / 1MB, 2)
            Write-Host "  → 大小: $sizeInMB MB" -ForegroundColor Gray
        } else {
            Write-Host "✗ 错误: 找不到编译输出文件 $sourcePath" -ForegroundColor Red
        }
    } else {
        Write-Host "✗ 编译失败: $targetName" -ForegroundColor Red
    }
}

# 创建压缩包
if (-not $SkipCompress) {
    Write-Host ""
    Write-Host "========================================" -ForegroundColor Cyan
    Write-Host "创建发布包..." -ForegroundColor Cyan
    Write-Host "========================================" -ForegroundColor Cyan
    
    foreach ($target in $targets.Keys) {
        $targetName = $targets[$target]
        
        if ($target -eq "x86_64-pc-windows-msvc") {
            $binaryFile = "bili-sync-rs-$targetName.exe"
            $archiveName = "bili-sync-rs-$targetName.zip"
        } else {
            $binaryFile = "bili-sync-rs-$targetName"
            $archiveName = "bili-sync-rs-$targetName.tar.gz"
        }
        
        $binaryPath = "$OutputDir\$binaryFile"
        if (Test-Path $binaryPath) {
            Write-Host "创建压缩包: $archiveName" -ForegroundColor Yellow
            
            if ($target -eq "x86_64-pc-windows-msvc") {
                # Windows 使用 ZIP
                $archivePath = "$OutputDir\$archiveName"
                Compress-Archive -Path $binaryPath -DestinationPath $archivePath -Force
                
                if (Test-Path $archivePath) {
                    $archiveSize = (Get-Item $archivePath).Length
                    $sizeInMB = [math]::Round($archiveSize / 1MB, 2)
                    Write-Host "  ✓ $archiveName ($sizeInMB MB)" -ForegroundColor Green
                }
            } else {
                # Linux/macOS 使用 tar.gz
                if (Get-Command tar -ErrorAction SilentlyContinue) {
                    Push-Location $OutputDir
                    tar -czf $archiveName $binaryFile
                    Pop-Location
                    
                    $archivePath = "$OutputDir\$archiveName"
                    if (Test-Path $archivePath) {
                        $archiveSize = (Get-Item $archivePath).Length
                        $sizeInMB = [math]::Round($archiveSize / 1MB, 2)
                        Write-Host "  ✓ $archiveName ($sizeInMB MB)" -ForegroundColor Green
                    }
                } else {
                    Write-Host "  ⚠ 警告: 未找到 tar 命令，跳过 $archiveName 创建" -ForegroundColor Yellow
                }
            }
        }
    }
}

Write-Host ""
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "编译完成！" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "成功编译: $successCount/$totalCount 个目标平台" -ForegroundColor Green
Write-Host "输出目录: $OutputDir" -ForegroundColor Gray
Write-Host ""
Write-Host "生成的文件:" -ForegroundColor Yellow
Get-ChildItem $OutputDir | ForEach-Object {
    $size = [math]::Round($_.Length / 1MB, 2)
    Write-Host "  $($_.Name) ($size MB)" -ForegroundColor Gray
}

Write-Host ""
Write-Host "使用方法:" -ForegroundColor Yellow
Write-Host "  1. 解压对应平台的压缩包" -ForegroundColor Gray
Write-Host "  2. 运行 bili-sync-rs 可执行文件" -ForegroundColor Gray
Write-Host "  3. 使用 --help 查看帮助信息" -ForegroundColor Gray 