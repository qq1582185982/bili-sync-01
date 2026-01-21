<script lang="ts">
	import { onMount } from 'svelte';
	import { setBreadcrumb } from '$lib/stores/breadcrumb';
	import * as Card from '$lib/components/ui/card';
	import { CHANGELOG_HTML, CHANGELOG_VERSION } from '$lib/generated/changelog';

	onMount(() => {
		setBreadcrumb([
			{ label: '总览', href: '/' },
			{ label: '更新记录', href: '/changelog' }
		]);
	});
</script>

<svelte:head>
	<title>更新记录 - Bili Sync</title>
</svelte:head>

<div class="space-y-6">
	<div class="flex items-start justify-between gap-4">
		<div>
			<h1 class="text-3xl font-bold tracking-tight">更新记录</h1>
			<p class="text-muted-foreground mt-2">查看 Bili Sync 的最新更新和改进记录</p>
		</div>
		<div class="text-right">
			<div class="text-muted-foreground text-xs">当前版本</div>
			<div class="font-mono text-sm">{CHANGELOG_VERSION}</div>
		</div>
	</div>

	<Card.Root class="min-h-[600px]">
		<Card.Content class="max-h-[calc(100vh-250px)] overflow-auto p-6">
			<article class="prose dark:prose-invert max-w-none">
				{@html CHANGELOG_HTML}
			</article>
		</Card.Content>
	</Card.Root>
</div>
