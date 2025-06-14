import { defineConfig } from "vitepress";
import taskLists from "markdown-it-task-lists";

// https://vitepress.dev/reference/site-config
export default defineConfig({
	title: "bili-sync",
	description: "由 Rust & Tokio 驱动的哔哩哔哩同步工具",
	lang: "zh-Hans",
	base: "/bili-sync-01/", // GitHub仓库名
	sitemap: {
		hostname: "https://qq1582185982.github.io/bili-sync-01",
	},
	lastUpdated: true,
	cleanUrls: true,
	metaChunk: true,
	ignoreDeadLinks: true, // 忽略死链接检查
	themeConfig: {
		outline: {
			label: "页面导航",
			level: "deep",
		},
		// https://vitepress.dev/reference/default-theme-config
		nav: [
			{ text: "主页", link: "/" },
			{
				        text: "v2.7.2 Final",
				items: [
					{
						text: "程序更新",
						link: "https://github.com/qq1582185982/bili-sync-01/releases",
					},
					{
						text: "文档更新",
						link: "https://github.com/qq1582185982/bili-sync-01/commits/main",
					},
				],
			},
		],
		sidebar: [
			{
				text: "入门",
				items: [
					{ text: "介绍", link: "/introduction" },
					{ text: "快速开始", link: "/quick-start" },
					{ text: "功能一览", link: "/features" },
				],
			},
			{
				text: "使用指南",
				collapsed: false,
				items: [
					{ text: "程序配置", link: "/configuration" },
					{ 
						text: "视频源管理",
						collapsed: true,
						items: [
							{ text: "管理指南", link: "/video-source-management" },
							{ text: "UP主投稿", link: "/submission" },
							{ text: "收藏夹", link: "/favorite" },
							{ text: "合集与系列", link: "/collection" },
							{ text: "番剧", link: "/bangumi" },
							{ text: "稍后观看", link: "/watch_later" },
						]
					},
					{ text: "命令行参数", link: "/args" },
				],
			},
			{
				text: "高级",
				items: [
					{ text: "任务队列管理", link: "/queue-management" },
					{ text: "智能风控处理", link: "/risk-control-guide" },
					{ text: "双重重置系统", link: "/reset-functionality-guide" },
					{ text: "视觉功能增强", link: "/visual-enhancements-guide" },
					{ text: "技术架构文档", link: "/technical-architecture" },
					{ text: "部署指南", link: "/deploy-guide" },
					{ text: "工作原理", link: "/design" },
				]
			},
			{
				text: "其它",
				items: [
					{ text: "常见问题", link: "/question" },
					{ text: "管理页(旧)", link: "/frontend" },
				],
			},
			{
				text: "更新日志",
				collapsed: true,
				items: [
					{ text: "v2.7.2 Final (2025-06-14)", link: "/update-log-2025-06-14-comprehensive" },
					{ text: "v2.7.2+ (2025-06-14)", link: "/update-log-2025-06-14" },
					{ text: "v2.7.1 (2025-06-08)", link: "/update-log-v2.7.1" },
					{ text: "v2.7.0 (2025-06-03)", link: "/comprehensive-update-2025-06-03" },
					{ text: "v2.6.2 (2025年6月)", link: "/update-log-2025-06" },
				],
			},
		],
		socialLinks: [
			{ icon: "github", link: "https://github.com/qq1582185982/bili-sync-01" },
		],
		search: {
			provider: "local",
		},
		notFound: {
			title: "你来到了没有知识的荒原",
			quote: "这里什么都没有",
			linkText: "返回首页",
		},
		docFooter: {
			prev: "上一页",
			next: "下一页",
		},
		lastUpdated: {
			text: "上次更新于",
		},
		returnToTopLabel: "回到顶部",
		sidebarMenuLabel: "菜单",
		darkModeSwitchLabel: "主题",
		lightModeSwitchTitle: "切换到浅色模式",
		darkModeSwitchTitle: "切换到深色模式",
	},
	markdown: {
		config: (md) => {
			md.use(taskLists);
		},
		theme: {
			light: "github-light",
			dark: "github-dark",
		},
	},
	head: [
		["link", { rel: "icon", type: "image/svg+xml", href: "/icon.svg" }],
		["link", { rel: "icon", type: "image/png", href: "/icon.png" }],
	],
});
