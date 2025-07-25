<script lang="ts">
	import '../app.css';
	import AppSidebar from '$lib/components/app-sidebar.svelte';
	import * as Sidebar from '$lib/components/ui/sidebar/index.js';
	import { goto } from '$app/navigation';
	import { Toaster } from '$lib/components/ui/sonner/index.js';
	import { breadcrumbStore } from '$lib/stores/breadcrumb';
	import BreadCrumb from '$lib/components/bread-crumb.svelte';
	import { videoSourceStore, setVideoSources } from '$lib/stores/video-source';
	import { onMount } from 'svelte';
	import api from '$lib/api';
	import { toast } from 'svelte-sonner';
	import type { ApiError } from '$lib/types';
	import { LogOut, BookOpen } from '@lucide/svelte';
	import ResponsiveButton from '$lib/components/responsive-button.svelte';
	import { initTheme } from '$lib/stores/theme';
	import ThemeToggle from '$lib/components/theme-toggle.svelte';

	let dataLoaded = false;
	let isAuthenticated = false;

	// 退出登录
	function handleLogout() {
		api.setAuthToken('');
		isAuthenticated = false;
		goto('/');
		window.location.reload(); // 重新加载页面以清除状态
	}

	// 检查认证状态
	async function checkAuthStatus() {
		const token = localStorage.getItem('auth_token');
		if (token) {
			api.setAuthToken(token);
			try {
				// 验证token有效性
				await api.getVideoSources();
				isAuthenticated = true;
				// 初始化视频源数据，所有组件都会用到
				if (!$videoSourceStore) {
					setVideoSources((await api.getVideoSources()).data);
				}
			} catch (error) {
				console.error('Token验证失败:', error);
				if ((error as ApiError).status === 401) {
					// Token 无效，清除
					isAuthenticated = false;
					api.setAuthToken('');
					localStorage.removeItem('auth_token');
				} else {
					// 只有在非401错误时才显示错误提示，避免新用户看到不必要的错误
					toast.error('加载视频来源失败', {
						description: (error as ApiError).message
					});
				}
			}
		} else {
			// 新用户没有token，这是正常情况，不需要显示错误
			isAuthenticated = false;
		}
		dataLoaded = true;
	}

	// 初始化共用数据
	onMount(async () => {
		// 初始化主题
		initTheme();
		await checkAuthStatus();
		// 监听登录成功事件
		window.addEventListener('login-success', () => {
			isAuthenticated = true;
			checkAuthStatus();
		});
	});
</script>

<Toaster />

<Sidebar.Provider>
	<div class="prevent-horizontal-scroll flex h-screen w-full overflow-hidden">
		{#if isAuthenticated}
			<div data-sidebar="sidebar">
				<AppSidebar />
			</div>
		{/if}
		<Sidebar.Inset class="flex h-screen flex-1 flex-col overflow-hidden">
			{#if isAuthenticated}
				<div
					class="bg-background/95 supports-[backdrop-filter]:bg-background/60 sticky top-0 z-50 flex min-h-[73px] w-full items-center border-b backdrop-blur"
				>
					<div class="flex w-full items-center gap-2 px-4 py-2 sm:gap-4 sm:px-6">
						<Sidebar.Trigger class="shrink-0" data-sidebar="trigger" />
						<div class="min-w-0 flex-1">
							<!-- 保留空间保持布局一致性 -->
						</div>
						<div class="flex items-center gap-1 sm:gap-2">
							<ThemeToggle />
							<ResponsiveButton
								size="sm"
								variant="outline"
								onclick={() => window.open('https://qq1582185982.github.io/bili-sync-01/', '_blank')}
								icon={BookOpen}
								text="文档"
								title="查看文档"
							/>
							<ResponsiveButton
								size="sm"
								variant="outline"
								onclick={handleLogout}
								icon={LogOut}
								text="退出"
								title="退出"
							/>
						</div>
					</div>
				</div>
			{/if}
			<div class="bg-background smooth-scroll flex-1 overflow-auto">
				<div class="w-full px-4 py-4 sm:px-6 sm:py-6">
					{#if isAuthenticated && $breadcrumbStore.length > 0}
						<div class="mb-6">
							<BreadCrumb items={$breadcrumbStore} />
						</div>
					{/if}
					{#if dataLoaded}
						<slot />
					{/if}
				</div>
			</div>
		</Sidebar.Inset>
	</div>
</Sidebar.Provider>
