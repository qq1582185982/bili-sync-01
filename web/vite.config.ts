import tailwindcss from '@tailwindcss/vite';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';
import { VitePWA } from 'vite-plugin-pwa';

export default defineConfig({
	plugins: [
		tailwindcss(),
		sveltekit(),
		VitePWA({
			registerType: 'autoUpdate',
			includeAssets: ['favicon.png', 'icon-192.png', 'icon-512.png'],
			manifest: {
				name: 'bili-sync 管理面板',
				short_name: 'bili-sync',
				description: 'B站视频同步下载管理工具',
				theme_color: '#ffffff',
				background_color: '#ffffff',
				display: 'standalone',
				scope: '/',
				start_url: '/',
				orientation: 'any',
				icons: [
					{
						src: '/icon-192.png',
						sizes: '192x192',
						type: 'image/png',
						purpose: 'any'
					},
					{
						src: '/icon-512.png',
						sizes: '512x512',
						type: 'image/png',
						purpose: 'any'
					},
					{
						src: '/icon-512.png',
						sizes: '512x512',
						type: 'image/png',
						purpose: 'maskable'
					}
				]
			},
			workbox: {
				// 运行时缓存策略
				runtimeCaching: [
					{
						// API请求使用NetworkFirst策略
						urlPattern: /^https?:\/\/.*\/api\/.*/i,
						handler: 'NetworkFirst',
						options: {
							cacheName: 'api-cache',
							expiration: {
								maxEntries: 100,
								maxAgeSeconds: 5 * 60 // 5分钟
							},
							networkTimeoutSeconds: 10
						}
					},
					{
						// 静态资源使用CacheFirst策略
						urlPattern: /\.(?:png|jpg|jpeg|svg|gif|webp|ico)$/i,
						handler: 'CacheFirst',
						options: {
							cacheName: 'images-cache',
							expiration: {
								maxEntries: 100,
								maxAgeSeconds: 30 * 24 * 60 * 60 // 30天
							}
						}
					},
					{
						// 视频流不缓存
						urlPattern: /\/api\/videos\/(proxy-stream|stream)/i,
						handler: 'NetworkOnly'
					}
				],
				// 排除不需要缓存的资源
				navigateFallback: null,
				// 清理过期缓存
				cleanupOutdatedCaches: true
			},
			devOptions: {
				enabled: true, // 开发环境也启用PWA
				type: 'module'
			}
		})
	],
	server: {
		proxy: {
			'/api': 'http://localhost:12345'
		}
	}
});
