# 📦 bili-sync 云端编译指南（小白版）

## 🎯 什么是 Artifacts？

**Artifacts** 就是编译好的程序文件，你可以直接下载使用，不需要自己编译代码。

简单来说：
- 你写代码 → GitHub 帮你编译 → 生成可执行文件 → 你下载使用

## 🚀 如何获取编译文件？

### 方法一：手动触发编译（推荐）

1. **打开你的 GitHub 仓库**
   ```
   https://github.com/qq1582185982/bili-sync-01
   ```

2. **进入 Actions 页面**
   - 点击仓库顶部的 "Actions" 标签

3. **选择编译工作流**
   - 点击左侧的 "Manual Build"

4. **开始编译**
   - 点击右侧的 "Run workflow" 按钮
   - 选择要编译的平台：
     - `all` - 编译所有平台（推荐）
     - `windows` - 只编译 Windows
     - `linux` - 只编译 Linux
     - `macos` - 只编译 macOS
   - 点击绿色的 "Run workflow" 按钮

5. **等待编译完成**
   - 编译时间：约 10-20 分钟
   - 状态会显示为 "Success" ✅

### 方法二：自动编译

每次你推送代码到 GitHub 时，会自动开始编译：

```bash
git add .
git commit -m "更新代码"
git push
```

## 📥 如何下载编译文件？

### 步骤 1：找到编译任务
1. 进入 "Actions" 页面
2. 点击最新的编译任务（状态为 Success ✅）

### 步骤 2：下载 Artifacts
在页面底部找到 "Artifacts" 部分，会看到这些文件：

| 文件名 | 适用系统 | 大小 |
|--------|----------|------|
| `bili-sync-rs-Windows-x86_64.zip` | Windows 64位 | ~10MB |
| `bili-sync-rs-Linux-x86_64-musl.tar.gz` | Linux 64位 | ~10MB |
| `bili-sync-rs-Linux-aarch64-musl.tar.gz` | Linux ARM64 | ~10MB |
| `bili-sync-rs-Darwin-x86_64.tar.gz` | macOS Intel | ~10MB |
| `bili-sync-rs-Darwin-aarch64.tar.gz` | macOS Apple Silicon | ~10MB |

### 步骤 3：选择你的系统
- **Windows 用户**：下载 `bili-sync-rs-Windows-x86_64.zip`
- **Linux 用户**：下载对应的 `.tar.gz` 文件
- **macOS 用户**：
  - Intel Mac：下载 `bili-sync-rs-Darwin-x86_64.tar.gz`
  - Apple Silicon (M1/M2)：下载 `bili-sync-rs-Darwin-aarch64.tar.gz`

## 📂 如何使用下载的文件？

### Windows 用户
1. **解压文件**
   - 右键点击 `.zip` 文件
   - 选择 "解压到当前文件夹"

2. **运行程序**
   ```cmd
   bili-sync-rs-Windows-x86_64.exe --help
   ```

### Linux/macOS 用户
1. **解压文件**
   ```bash
   tar -xzf bili-sync-rs-Linux-x86_64-musl.tar.gz
   ```

2. **添加执行权限**
   ```bash
   chmod +x bili-sync-rs-Linux-x86_64-musl
   ```

3. **运行程序**
   ```bash
   ./bili-sync-rs-Linux-x86_64-musl --help
   ```

## ⏰ 编译时间说明

| 编译类型 | 时间 | 说明 |
|----------|------|------|
| 首次编译 | 15-20分钟 | 需要下载依赖 |
| 后续编译 | 8-12分钟 | 使用缓存，更快 |
| 单平台编译 | 3-8分钟 | 只编译一个平台 |

## 🔍 编译状态说明

| 状态 | 图标 | 说明 |
|------|------|------|
| In progress | 🟡 | 正在编译中 |
| Success | ✅ | 编译成功 |
| Failed | ❌ | 编译失败 |
| Cancelled | ⚪ | 编译被取消 |

## 🚨 常见问题

### Q: 为什么编译失败了？
**A:** 常见原因：
- 代码有语法错误
- 依赖下载失败
- 网络问题

**解决方法：**
1. 检查代码是否有错误
2. 重新运行编译
3. 查看详细错误日志

### Q: 下载的文件无法运行？
**A:** 检查：
- 是否下载了正确的平台版本
- Linux/macOS 是否添加了执行权限
- 是否解压了文件

### Q: 编译时间太长？
**A:** 优化方法：
- 使用 "Manual Build" 只编译需要的平台
- 避免频繁推送代码
- 首次编译会比较慢，后续会快很多

### Q: 找不到 Artifacts？
**A:** 确认：
- 编译任务是否完成（状态为 Success）
- 是否在正确的编译任务页面
- Artifacts 有 90 天过期时间

## 💡 小贴士

### 🎯 推荐工作流程
1. **开发阶段**：本地编译 Windows 版本测试
2. **发布阶段**：使用 GitHub Actions 编译全平台版本
3. **紧急修复**：使用手动编译快速生成文件

### 🔧 提高效率
- 使用 "Manual Build" 可以选择特定平台
- 合并多个小改动后再推送，减少编译次数
- 利用缓存机制，相同代码编译会更快

### 📱 移动端查看
GitHub Actions 在手机上也能查看：
1. 打开 GitHub App
2. 进入你的仓库
3. 点击 "Actions" 标签
4. 查看编译状态和下载文件

## 🎉 总结

使用 GitHub Actions 编译的好处：
- ✅ **无需配置环境**：GitHub 提供现成的编译环境
- ✅ **支持全平台**：一次编译，生成 5 个平台的版本
- ✅ **自动化**：推送代码即可触发编译
- ✅ **免费**：对公开仓库完全免费
- ✅ **稳定可靠**：统一的编译环境，避免本地问题

现在你可以轻松获得全平台的 bili-sync 编译版本了！🚀

---

**需要帮助？**
- 查看 [GitHub Actions 使用说明](github-actions-build.md)
- 在仓库中提交 Issue
- 查看编译日志获取详细错误信息 