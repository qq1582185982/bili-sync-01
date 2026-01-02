export default {
  title: 'Bili-Sync',
  description: '由 Rust & Tokio 驱动的哔哩哔哩同步工具（升级优化版）',
  base: '/bili-sync-up/',
  ignoreDeadLinks: true,

  themeConfig: {
    logo: '/icon.svg',

    nav: [
      { text: '首页', link: '/' },
      { text: '使用指南', link: '/installation' },
      { text: 'GitHub', link: 'https://github.com/NeeYoonc/bili-sync-up' }
    ],

    sidebar: [
      {
        text: '开始使用',
        items: [
          { text: '安装指南', link: '/installation' },
          { text: '使用教程', link: '/usage' }
        ]
      },
      {
        text: '参考',
        items: [
          { text: '功能列表', link: '/features' },
          { text: '常见问题', link: '/faq' },
          { text: '更新记录', link: '/changelog' }
        ]
      }
    ],

    socialLinks: [
      { icon: 'github', link: 'https://github.com/NeeYoonc/bili-sync-up' }
    ],

    footer: {
      copyright: 'Copyright © 2025 Bili-Sync'
    }
  }
}