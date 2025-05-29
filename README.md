![bili-sync](https://socialify.git.ci/amtoaer/bili-sync/image?description=1&font=KoHo&issues=1&language=1&logo=https%3A%2F%2Fs2.loli.net%2F2023%2F12%2F02%2F9EwT2yInOu1d3zm.png&name=1&owner=1&pattern=Signal&pulls=1&stargazers=1&theme=Light)

## 简介

> [!NOTE]
> [点击此处](https://bili-sync.allwens.work/)查看文档

bili-sync 是一款专为 NAS 用户编写的哔哩哔哩同步工具，由 Rust & Tokio 驱动。

## 🌟 项目亮点

### 🆕 相比原版本的主要改进

- **🎨 Web 管理界面增强** - 可直接在管理页面添加视频源，告别手动编辑配置文件
- **📺 番剧下载支持** - 完整支持番剧下载，包括单季和全季模式
- **⚙️ 配置系统升级** - 新的 V2 配置格式，支持运行时修改，更加灵活
- **🐳 Docker 部署优化** - 多阶段构建、健康检查、更好的缓存策略
- **🛠️ 开发体验提升** - 完整的开发工具链和详细文档
- **🔧 智能任务调度** - 自动避免数据库锁定冲突，提高稳定性

### 💡 技术优势

- **高性能**: Rust + Tokio 异步架构，支持高并发下载
- **易部署**: 提供多平台二进制文件和 Docker 镜像
- **易使用**: 友好的 Web 管理界面，无需命令行操作
- **易维护**: 清晰的项目结构和完整的开发文档
- **高稳定**: 智能错误处理和自动重试机制

## 快速开始

### 开发环境设置

```bash
# 克隆项目
git clone <repository-url>
cd bili-sync

# 方法 1: 使用批处理文件 (推荐)
.\make.bat setup

# 方法 2: 使用 PowerShell 脚本
# 首先设置执行策略
Set-ExecutionPolicy -ExecutionPolicy Bypass -Scope Process
.\make.ps1 setup

# 启动开发服务器
.\make.bat dev
# 或
.\make.ps1 dev
```

### 常用命令

```bash
# 查看所有可用任务
.\make.bat help

# 运行测试
.\make.bat test

# 构建项目
.\make.bat build

# 构建发布版本
.\make.bat release

# 清理构建文件
.\make.bat clean
```

> **注意**: 如果您遇到 PowerShell 执行策略问题，建议使用 `.\make.bat` 命令，或者先运行 `Set-ExecutionPolicy -ExecutionPolicy Bypass -Scope Process` 来临时允许脚本执行。

## 项目结构

```
├── crates/                 # Rust 后端代码
│   ├── bili_sync/          # 主应用程序
│   ├── bili_sync_entity/   # 数据库实体定义
│   └── bili_sync_migration/# 数据库迁移脚本
├── web/                    # Svelte 前端应用
├── docs/                   # VitePress 文档站点
├── scripts/                # 辅助工具脚本
├── assets/                 # 项目资源文件
├── make.bat               # Windows 批处理任务脚本
└── make.ps1               # PowerShell 任务脚本
```

详细的项目结构说明请查看 [PROJECT_STRUCTURE.md](./PROJECT_STRUCTURE.md)

## 效果演示

### 概览
![概览](./assets/overview.webp)
### 详情
![详情](./assets/detail.webp)
### 播放（使用 infuse）
![播放](./assets/play.webp)
### 文件排布
![文件](./assets/dir.webp)

## 功能与路线图

### 🎯 核心功能
- [x] 使用用户填写的凭据认证，并在必要时自动刷新
- [x] **可以在管理页面 `0.0.0.0:12345` 内添加视频源** 🆕
- [x] 支持收藏夹与视频列表/视频合集的下载
- [x] **支持番剧的下载，包括单季模式和全季模式** 🆕
- [x] 自动选择用户设置范围内最优的视频和音频流，并在下载完成后使用 FFmpeg 合并          
- [x] 使用 Tokio 与 Reqwest，对视频、视频分页进行异步并发下载
- [x] 使用媒体服务器支持的文件命名，方便一键作为媒体库导入
- [x] 当前轮次下载失败会在下一轮下载时重试，失败次数过多自动丢弃
- [x] 使用数据库保存媒体信息，避免对同个视频的多次请求
- [x] 打印日志，并在请求出现风控时自动终止，等待下一轮执行
- [x] 提供多平台的二进制可执行文件，为 Linux 平台提供了立即可用的 Docker 镜像
- [x] 支持对"稍后再看"内视频的自动扫描与下载
- [x] 支持对 UP 主投稿视频的自动扫描与下载
- [x] 支持限制任务的并行度和接口请求频率

### 🚀 新增特性
- [x] **Web 管理界面增强** - 通过友好的 Web 界面直接管理视频源，无需手动编辑配置文件
- [x] **番剧下载支持** - 完整支持番剧下载，包括单季和全季模式
- [x] **配置文件格式升级** - 新的 V2 配置格式，支持更灵活的配置选项
- [x] **Docker 部署优化** - 多阶段构建、健康检查、更好的缓存策略
- [x] **开发工具链改进** - 完整的开发环境设置和构建脚本
- [x] **智能任务调度** - 重命名操作时自动暂停扫描任务，避免数据库锁定冲突

### 🔄 持续改进
- [x] **项目结构优化** - 更清晰的代码组织和技术栈说明
- [x] **UI 组件扩展** - 新增对话框、标签、选择器等交互组件
- [x] **配置管理增强** - 统一的 `config.toml` 配置文件，支持运行时修改
- [x] **文档和开发体验** - 详细的快速开始指南和贡献指南

### 📋 开发路线图
- [ ] 下载单个文件时支持断点续传与并发下载
- [ ] 更多媒体服务器兼容性优化
- [ ] 高级过滤和搜索功能
- [ ] 批量操作和管理功能

## 开发指南

### 技术栈

- **后端**: Rust + Tokio + Axum + SeaORM
- **前端**: Svelte + TypeScript + Tailwind CSS
- **数据库**: SQLite
- **文档**: VitePress
- **部署**: Docker + Docker Compose

### 开发流程

1. **环境准备**: 确保安装了 Rust 和 Node.js
2. **依赖安装**: 运行 `.\make.bat setup`
3. **启动开发**: 运行 `.\make.bat dev`
4. **代码检查**: 运行 `.\make.bat lint`
5. **运行测试**: 运行 `.\make.bat test`

### 可用的任务脚本

项目提供了多种任务脚本供您选择：

- **`make.bat`** - Windows 批处理文件，无需特殊权限
- **`make.ps1`** - PowerShell 脚本，功能更丰富

### 贡献指南

1. Fork 本项目
2. 创建功能分支 (`git checkout -b feature/AmazingFeature`)
3. 提交更改 (`git commit -m 'Add some AmazingFeature'`)
4. 推送到分支 (`git push origin feature/AmazingFeature`)
5. 创建 Pull Request

## 参考与借鉴

该项目实现过程中主要参考借鉴了如下的项目，感谢他们的贡献：

+ [bilibili-API-collect](https://github.com/SocialSisterYi/bilibili-API-collect) B 站的第三方接口文档
+ [bilibili-api](https://github.com/Nemo2011/bilibili-api) 使用 Python 调用接口的参考实现
+ [danmu2ass](https://github.com/gwy15/danmu2ass) 本项目弹幕下载功能的缝合来源
