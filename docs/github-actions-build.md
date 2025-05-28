# GitHub Actions 云端编译指南

## 📋 概述

使用 GitHub Actions 可以在云端自动编译所有平台的 bili-sync 版本，无需本地安装复杂的交叉编译工具链。

## 🚀 快速开始

### 1. 上传代码到 GitHub

```bash
# 如果还没有 Git 仓库
git init
git add .
git commit -m "Initial commit"

# 推送到 GitHub（替换为你的仓库地址）
git remote add origin https://github.com/你的用户名/bili-sync.git
git push -u origin main
```

### 2. 触发编译

有三种方式触发编译：

#### 方式一：手动触发（推荐）
1. 打开你的 GitHub 仓库
2. 点击 `Actions` 标签页
3. 选择 `Manual Build` 工作流
4. 点击 `Run workflow`
5. 选择要编译的平台：
   - `all` - 编译所有平台
   - `windows` - 只编译 Windows
   - `linux` - 只编译 Linux
   - `macos` - 只编译 macOS

#### 方式二：推送代码自动触发
```bash
git add .
git commit -m "更新代码"
git push
```

#### 方式三：创建版本标签
```bash
git tag v2.5.1
git push origin v2.5.1
```
这会自动创建 GitHub Release 并上传所有编译文件。

## 📦 编译结果

### 编译平台
- ✅ **Windows x86_64** - `bili-sync-rs-Windows-x86_64.zip`
- ✅ **Linux x86_64** - `bili-sync-rs-Linux-x86_64-musl.tar.gz`
- ✅ **Linux ARM64** - `bili-sync-rs-Linux-aarch64-musl.tar.gz`
- ✅ **macOS x86_64** - `bili-sync-rs-Darwin-x86_64.tar.gz`
- ✅ **macOS ARM64** - `bili-sync-rs-Darwin-aarch64.tar.gz`

### 下载编译结果

#### 从 Actions 页面下载
1. 进入 `Actions` 标签页
2. 点击最新的编译任务
3. 在 `Artifacts` 部分下载对应平台的文件

#### 从 Releases 页面下载（仅限标签触发）
1. 进入 `Releases` 标签页
2. 下载最新版本的文件

## ⚙️ 工作流说明

### build.yml - 完整编译工作流
- **触发条件**：推送到 main/master 分支、创建标签、手动触发
- **功能**：编译所有平台，创建 Release（仅标签触发）
- **时间**：约 15-20 分钟

### manual-build.yml - 手动编译工作流
- **触发条件**：仅手动触发
- **功能**：可选择编译特定平台
- **时间**：约 5-15 分钟（取决于选择的平台）

## 🔧 自定义配置

### 修改编译目标
编辑 `.github/workflows/build.yml` 文件中的 `matrix.include` 部分：

```yaml
matrix:
  include:
    - target: x86_64-pc-windows-msvc
      os: windows-latest
      name: Windows-x86_64
    # 添加或删除其他平台...
```

### 修改触发条件
编辑工作流文件中的 `on` 部分：

```yaml
on:
  push:
    branches: [ main ]  # 只在推送到 main 分支时触发
  workflow_dispatch:    # 允许手动触发
```

## 💡 优势

### 相比本地编译
- ✅ **无需配置复杂环境**：GitHub 提供预配置的编译环境
- ✅ **并行编译**：5个平台同时编译，节省时间
- ✅ **稳定可靠**：统一的编译环境，避免本地环境问题
- ✅ **自动化**：推送代码即可触发编译
- ✅ **免费**：GitHub Actions 对公开仓库免费

### 编译时间对比
- **本地编译**：每个平台 5-10 分钟，总计 25-50 分钟
- **GitHub Actions**：并行编译，总计 15-20 分钟

## 🚨 注意事项

1. **仓库必须是公开的**，或者有 GitHub Pro 账户
2. **每月有免费额度限制**：
   - 公开仓库：无限制
   - 私有仓库：2000 分钟/月
3. **编译失败时**：检查 Actions 页面的错误日志
4. **首次编译较慢**：需要下载依赖，后续编译会使用缓存

## 🔍 故障排除

### 编译失败
1. 检查 Actions 页面的详细日志
2. 确认代码没有语法错误
3. 检查 `web/package.json` 是否存在

### 下载不到文件
1. 确认编译任务已完成
2. 检查是否有权限访问仓库
3. Artifacts 有 90 天过期时间

### 想要更快的编译
1. 使用 `manual-build.yml` 只编译需要的平台
2. 考虑使用自托管的 GitHub Runner

## 📚 相关链接

- [GitHub Actions 文档](https://docs.github.com/en/actions)
- [Rust 交叉编译指南](https://rust-lang.github.io/rustup/cross-compilation.html)
- [cross 工具文档](https://github.com/cross-rs/cross) 