# 🚀 Artifacts 快速参考卡片

## 📱 一分钟获取编译文件

### 🎯 快速步骤
1. 打开：https://github.com/qq1582185982/bili-sync-01
2. 点击：**Actions** 标签
3. 选择：**Manual Build**
4. 点击：**Run workflow** → 选择平台 → **Run workflow**
5. 等待：10-20分钟
6. 下载：页面底部的 **Artifacts**

### 📦 文件对照表

| 你的系统 | 下载文件 |
|----------|----------|
| Windows | `bili-sync-rs-Windows-x86_64.zip` |
| Linux | `bili-sync-rs-Linux-x86_64-musl.tar.gz` |
| Linux ARM | `bili-sync-rs-Linux-aarch64-musl.tar.gz` |
| macOS Intel | `bili-sync-rs-Darwin-x86_64.tar.gz` |
| macOS M1/M2 | `bili-sync-rs-Darwin-aarch64.tar.gz` |

### ⚡ 使用方法

**Windows:**
```cmd
# 解压后直接运行
bili-sync-rs-Windows-x86_64.exe --help
```

**Linux/macOS:**
```bash
# 解压
tar -xzf 文件名.tar.gz
# 添加权限
chmod +x 程序名
# 运行
./程序名 --help
```

### 🔍 状态图标
- 🟡 编译中
- ✅ 成功（可下载）
- ❌ 失败
- ⚪ 取消

### 💡 小技巧
- 选择 `all` 编译所有平台
- 选择具体平台编译更快
- Artifacts 保存 90 天
- 手机也能查看和下载

---
**完整文档：** [artifacts-guide.md](artifacts-guide.md) 