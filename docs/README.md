# 📚 bili-sync 文档中心

> 欢迎来到 bili-sync 文档中心！这里有你需要的所有编译和使用指南。

## 🎯 快速导航

### 🚀 Artifacts 编译文档（推荐新手）

| 文档 | 适用人群 | 内容 |
|------|----------|------|
| [📦 Artifacts 完整指南](artifacts-guide.md) | 所有用户 | 详细的编译和下载教程 |
| [🚀 快速参考卡片](artifacts-quick-reference.md) | 熟练用户 | 一页纸快速操作指南 |
| [📸 操作截图指南](artifacts-screenshots-guide.md) | 小白用户 | 图文并茂的详细步骤 |

### 🔧 技术文档

| 文档 | 适用人群 | 内容 |
|------|----------|------|
| [GitHub Actions 使用说明](github-actions-build.md) | 开发者 | 云端编译配置和原理 |
| [本地编译指南](../cross-compile.bat) | 开发者 | 本地跨平台编译脚本 |
| [Docker 构建指南](../Dockerfile) | 运维人员 | 容器化部署方案 |

## 🎯 我是小白，应该看哪个？

### 第一次使用？
👉 **推荐阅读顺序：**
1. [📸 操作截图指南](artifacts-screenshots-guide.md) - 图文并茂，最容易理解
2. [🚀 快速参考卡片](artifacts-quick-reference.md) - 保存备用，随时查看

### 已经会基本操作？
👉 **直接使用：**
- [🚀 快速参考卡片](artifacts-quick-reference.md) - 一分钟获取编译文件

### 想了解更多细节？
👉 **深入学习：**
- [📦 Artifacts 完整指南](artifacts-guide.md) - 包含所有细节和问题解答

## 🎯 常见使用场景

### 🔥 场景一：我想要现成的程序
**解决方案：** 使用 GitHub Actions 云端编译
- 📖 阅读：[操作截图指南](artifacts-screenshots-guide.md)
- ⚡ 快速：[快速参考卡片](artifacts-quick-reference.md)

### 🔥 场景二：我想修改代码后编译
**解决方案：** 修改代码 → 推送 → 自动编译
- 📖 阅读：[Artifacts 完整指南](artifacts-guide.md)
- 🔧 技术：[GitHub Actions 说明](github-actions-build.md)

### 🔥 场景三：我想在本地编译
**解决方案：** 使用本地编译脚本
- 🔧 Windows：运行 `cross-compile.bat`
- 🔧 跨平台：查看 [本地编译指南](../cross-compile.bat)

### 🔥 场景四：我想部署到服务器
**解决方案：** 使用 Docker 容器
- 🐳 Docker：查看 [Dockerfile](../Dockerfile)
- 📖 说明：[Docker 构建指南](../README.md)

## 📱 移动端用户

**好消息！** GitHub Actions 支持手机操作：

1. **下载 GitHub App**
2. **登录账号**
3. **进入仓库 → Actions**
4. **触发编译和查看状态**
5. **在浏览器中下载文件**

详细步骤见：[操作截图指南](artifacts-screenshots-guide.md)

## 🚨 遇到问题？

### 📋 问题排查清单

**编译问题：**
- ✅ 检查编译状态是否为 Success
- ✅ 查看详细错误日志
- ✅ 尝试重新运行编译

**下载问题：**
- ✅ 确认编译已完成
- ✅ 检查 Artifacts 是否存在
- ✅ 确认文件未过期（90天）

**运行问题：**
- ✅ 下载了正确的平台版本
- ✅ 完整解压了文件
- ✅ Linux/macOS 添加了执行权限

### 📞 获取帮助

1. **查看文档**：先查看相关文档
2. **搜索 Issues**：在仓库中搜索类似问题
3. **提交 Issue**：描述详细的问题和错误信息
4. **查看日志**：提供编译或运行的错误日志

## 🎉 成功案例

### ✅ 编译成功的标志
- 所有平台显示 ✅ Success
- Artifacts 部分有 5 个文件
- 文件大小约 9-20 MB

### ✅ 正确的文件名
```
bili-sync-rs-Windows-x86_64.zip        (19.8 MB)
bili-sync-rs-Linux-x86_64-musl.tar.gz  (10 MB)
bili-sync-rs-Linux-aarch64-musl.tar.gz (9.35 MB)
bili-sync-rs-Darwin-x86_64.tar.gz      (9.67 MB)
bili-sync-rs-Darwin-aarch64.tar.gz     (9.3 MB)
```

## 🔄 文档更新

**最后更新：** 2024年12月

**更新内容：**
- ✅ 添加了完整的 Artifacts 编译指南
- ✅ 创建了图文并茂的操作说明
- ✅ 提供了快速参考卡片
- ✅ 修复了文件名显示问题
- ✅ 优化了编译工作流

---

## 🚀 开始使用

**新手推荐路径：**
1. 📸 [操作截图指南](artifacts-screenshots-guide.md) - 跟着图片一步步操作
2. 🚀 [快速参考卡片](artifacts-quick-reference.md) - 保存到收藏夹
3. 🎯 开始你的第一次编译！

**祝你使用愉快！** 🎉

---

**项目地址：** https://github.com/qq1582185982/bili-sync-01  
**编译地址：** https://github.com/qq1582185982/bili-sync-01/actions 