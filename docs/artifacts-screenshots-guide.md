# 📸 Artifacts 操作截图指南

> 这是一个图文并茂的操作指南，帮助小白用户轻松获取编译文件

## 🎯 第一步：进入 GitHub 仓库

**网址：** https://github.com/qq1582185982/bili-sync-01

```
在浏览器中打开上面的链接，你会看到项目主页
```

**要点：**
- 确保你已经登录 GitHub 账号
- 如果是你自己的仓库，你会看到更多操作选项

---

## 🎯 第二步：进入 Actions 页面

**操作：** 点击页面顶部的 "Actions" 标签

```
GitHub 仓库页面顶部有几个标签：
< > Code    Issues    Pull requests    Actions    Projects    Wiki    Security    Insights    Settings
                                        ↑
                                    点击这里
```

**你会看到：**
- 左侧：工作流列表
- 右侧：最近的编译记录
- 如果是第一次，可能没有编译记录

---

## 🎯 第三步：选择编译工作流

**操作：** 在左侧找到并点击 "Manual Build"

```
左侧工作流列表：
├── All workflows
├── Build bili-sync          ← 自动编译
└── Manual Build            ← 手动编译（点击这个）
```

**说明：**
- **Manual Build**：手动触发，可以选择平台
- **Build bili-sync**：自动触发，编译所有平台

---

## 🎯 第四步：开始编译

**操作：** 点击右侧的 "Run workflow" 按钮

```
页面右侧会出现一个绿色按钮：
┌─────────────────┐
│   Run workflow  │  ← 点击这个按钮
└─────────────────┘
```

**弹出选项：**
```
┌─────────────────────────────┐
│ Use workflow from           │
│ Branch: main               │
│                            │
│ 选择要编译的平台             │
│ ┌─────────────────────────┐ │
│ │ all                    ▼│ │  ← 点击下拉菜单
│ └─────────────────────────┘ │
│                            │
│ ┌─────────────────────────┐ │
│ │     Run workflow        │ │  ← 最后点击这个
│ └─────────────────────────┘ │
└─────────────────────────────┘
```

**平台选项：**
- `all` - 编译所有平台（推荐新手）
- `windows` - 只编译 Windows
- `linux` - 只编译 Linux
- `macos` - 只编译 macOS

---

## 🎯 第五步：等待编译完成

**编译状态显示：**

```
🟡 正在编译中：
┌─────────────────────────────────────┐
│ Manual Build #1                     │
│ 🟡 In progress                      │
│ Started 2 minutes ago               │
└─────────────────────────────────────┘

✅ 编译成功：
┌─────────────────────────────────────┐
│ Manual Build #1                     │
│ ✅ Success                          │
│ Completed 1 minute ago              │
└─────────────────────────────────────┘

❌ 编译失败：
┌─────────────────────────────────────┐
│ Manual Build #1                     │
│ ❌ Failed                           │
│ Failed 30 seconds ago               │
└─────────────────────────────────────┘
```

**编译时间：**
- 首次编译：15-20 分钟
- 后续编译：8-12 分钟
- 单平台编译：3-8 分钟

---

## 🎯 第六步：下载编译文件

**操作：** 点击编译成功的任务

```
点击状态为 ✅ Success 的编译任务
```

**在任务详情页面：**

1. **查看编译进度**
   ```
   页面中间会显示 5 个编译任务：
   ✅ Build Windows-x86_64
   ✅ Build Linux-x86_64-musl  
   ✅ Build Linux-aarch64-musl
   ✅ Build Darwin-x86_64
   ✅ Build Darwin-aarch64
   ```

2. **找到 Artifacts 部分**
   ```
   滚动到页面底部，找到：
   
   📦 Artifacts
   ┌─────────────────────────────────────────────┐
   │ bili-sync-rs-Windows-x86_64.zip    19.8 MB │ ← 点击下载
   │ bili-sync-rs-Linux-x86_64-musl.tar.gz 10 MB│
   │ bili-sync-rs-Linux-aarch64-musl.tar.gz 9.35 MB│
   │ bili-sync-rs-Darwin-x86_64.tar.gz  9.67 MB │
   │ bili-sync-rs-Darwin-aarch64.tar.gz 9.3 MB  │
   └─────────────────────────────────────────────┘
   ```

3. **选择你的平台文件**
   - Windows 用户：点击 `.zip` 文件
   - Linux 用户：点击对应的 `.tar.gz` 文件
   - macOS 用户：根据你的 Mac 类型选择

---

## 🎯 第七步：使用下载的文件

### Windows 用户

1. **解压文件**
   ```
   右键点击下载的 .zip 文件
   ↓
   选择 "解压到当前文件夹" 或 "Extract Here"
   ↓
   得到 bili-sync-rs-Windows-x86_64.exe 文件
   ```

2. **运行程序**
   ```cmd
   # 在文件所在目录打开命令提示符
   # 输入以下命令测试：
   bili-sync-rs-Windows-x86_64.exe --help
   ```

### Linux/macOS 用户

1. **解压文件**
   ```bash
   # 在终端中运行：
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

---

## 🚨 常见问题解决

### ❓ 找不到 "Run workflow" 按钮？

**可能原因：**
- 你不是仓库的所有者或协作者
- 你没有登录 GitHub
- 工作流文件有问题

**解决方法：**
1. 确保已登录 GitHub
2. 检查是否有仓库权限
3. 尝试刷新页面

### ❓ 编译失败了怎么办？

**查看错误信息：**
1. 点击失败的编译任务
2. 点击红色的 ❌ 失败步骤
3. 查看详细错误日志

**常见解决方法：**
- 重新运行编译
- 检查代码是否有语法错误
- 等待一段时间后重试（可能是网络问题）

### ❓ 下载的文件无法运行？

**检查清单：**
- ✅ 是否下载了正确的平台版本？
- ✅ Windows 文件是否被杀毒软件拦截？
- ✅ Linux/macOS 是否添加了执行权限？
- ✅ 是否完整解压了文件？

### ❓ Artifacts 找不到或过期？

**说明：**
- Artifacts 保存期限：90 天
- 只有编译成功的任务才有 Artifacts
- 编译中的任务不会显示 Artifacts

**解决方法：**
- 重新运行编译
- 检查编译状态是否为 Success

---

## 💡 高级技巧

### 🔄 自动编译触发

每次推送代码时自动编译：
```bash
git add .
git commit -m "更新功能"
git push
```

### 📱 手机端操作

1. 下载 GitHub App
2. 登录你的账号
3. 进入仓库 → Actions
4. 查看编译状态
5. 下载 Artifacts（需要在浏览器中打开）

### ⚡ 快速重新编译

如果编译失败：
1. 进入失败的编译任务
2. 点击右上角的 "Re-run all jobs"
3. 等待重新编译完成

---

## 🎉 恭喜！

现在你已经学会了：
- ✅ 如何触发云端编译
- ✅ 如何查看编译状态  
- ✅ 如何下载编译文件
- ✅ 如何使用下载的程序
- ✅ 如何解决常见问题

享受你的 bili-sync 全平台版本吧！🚀

---

**相关文档：**
- [完整指南](artifacts-guide.md)
- [快速参考](artifacts-quick-reference.md)
- [GitHub Actions 说明](github-actions-build.md) 