---
# https://vitepress.dev/reference/default-theme-home-page
layout: home

title: bili-sync
titleTemplate: 由 Rust & Tokio 驱动的哔哩哔哩同步工具

hero:
  name: "bili-sync"
  text: "由 Rust & Tokio 驱动的哔哩哔哩同步工具"
  tagline: "v2.7.2 Final - 智能化系统，真正零干预的下载体验"
  actions:
    - theme: brand
      text: 什么是 bili-sync？
      link: /introduction
    - theme: alt
      text: 快速开始
      link: /quick-start
    - theme: alt
      text: GitHub
      link: https://github.com/qq1582185982/bili-sync-01
  image:
    src: /logo.webp
    alt: bili-sync

features:
  - icon: 🤖
    title: 智能风控处理系统
    details: 革命性突破！自动检测、处理、恢复风控，用户完全无感知的零干预体验
  - icon: 🔄
    title: 双重重置系统
    details: 自动重置 + 精确手动重置的完美结合，智能保护已完成内容
  - icon: 🖼️
    title: 完美视觉体验
    details: 图片代理技术解决防盗链，动态分页智能适配，现代化界面设计
  - icon: 🎛️
    title: 智能视频源管理
    details: 启用/禁用功能精确控制扫描，智能任务队列避免冲突
  - icon: 💾
    title: 专为 NAS 设计
    details: 可被 Emby、Jellyfin 等媒体服务器一键识别，完整的元数据支持
  - icon: 🐳
    title: 部署简单
    details: 提供简单易用的 docker 镜像，支持多架构部署
---

<style>
:root {
  --vp-home-hero-name-color: transparent;
  --vp-home-hero-name-background: -webkit-linear-gradient(120deg, #bd34fe 30%, #41d1ff);

  --vp-home-hero-image-background-image: linear-gradient(-45deg, #bd34fe 50%, #47caff 50%);
  --vp-home-hero-image-filter: blur(44px);
}

@media (min-width: 640px) {
  :root {
    --vp-home-hero-image-filter: blur(56px);
  }
}

@media (min-width: 960px) {
  :root {
    --vp-home-hero-image-filter: blur(68px);
  }
}
</style>