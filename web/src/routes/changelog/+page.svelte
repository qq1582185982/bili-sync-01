<script lang="ts">
	import { onMount } from 'svelte';
	import { setBreadcrumb } from '$lib/stores/breadcrumb';
	import * as Card from '$lib/components/ui/card';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import { AlertTriangle, Loader2 } from '@lucide/svelte';

	let loading = true;
	let error = false;
	let iframeElement: HTMLIFrameElement;

	// 设置面包屑导航
	onMount(() => {
		setBreadcrumb([
			{ name: '总览', href: '/' },
			{ name: '更新记录', href: '/changelog' }
		]);

		// 处理iframe加载完成
		if (iframeElement) {
			iframeElement.onload = () => {
				loading = false;
			};
			iframeElement.onerror = () => {
				loading = false;
				error = true;
			};
		}
	});

	// 重试加载
	function retryLoad() {
		loading = true;
		error = false;
		if (iframeElement) {
			iframeElement.src = iframeElement.src;
		}
	}
</script>

<svelte:head>
	<title>更新日志 - Bili Sync</title>
</svelte:head>

<div class="space-y-6">
	<div>
		<h1 class="text-3xl font-bold tracking-tight">更新记录</h1>
		<p class="text-muted-foreground mt-2">查看 Bili Sync 的最新更新和改进记录</p>
	</div>

	<Card.Root class="h-[calc(100vh-250px)] min-h-[600px]">
		<Card.Content class="relative h-full p-0">
			{#if loading}
				<div class="bg-background absolute inset-0 flex flex-col items-center justify-center gap-4">
					<Loader2 class="h-8 w-8 animate-spin" />
					<div class="space-y-2 text-center">
						<Skeleton class="h-4 w-48" />
						<Skeleton class="h-4 w-32" />
					</div>
					<p class="text-muted-foreground text-sm">正在加载更新记录...</p>
				</div>
			{/if}

			{#if error}
				<div class="bg-background absolute inset-0 flex flex-col items-center justify-center gap-4">
					<AlertTriangle class="text-destructive h-16 w-16" />
					<div class="space-y-2 text-center">
						<h3 class="text-lg font-semibold">加载失败</h3>
						<p class="text-muted-foreground text-sm">无法加载更新记录内容，请检查网络连接</p>
						<button
							class="bg-primary text-primary-foreground hover:bg-primary/90 mt-4 rounded-md px-4 py-2 transition-colors"
							on:click={retryLoad}
						>
							重试
						</button>
					</div>
				</div>
			{/if}

			<iframe
				bind:this={iframeElement}
				src="https://NeeYoonc.github.io/bili-sync-up/changelog.html"
				title="更新日志"
				class="h-full w-full rounded-lg border-0"
				class:opacity-0={loading || error}
				class:opacity-100={!loading && !error}
				frameborder="0"
				sandbox="allow-scripts allow-same-origin allow-popups allow-popups-to-escape-sandbox"
			></iframe>
		</Card.Content>
	</Card.Root>
</div>

<style>
	iframe {
		transition: opacity 0.3s ease-in-out;
	}
</style>
