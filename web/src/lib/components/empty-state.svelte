<script lang="ts">
	import type { Snippet } from 'svelte';

	// eslint-disable-next-line @typescript-eslint/no-explicit-any
	type IconComponent = any;

	let {
		icon = null,
		title = '暂无内容',
		description = '',
		iconClass = 'h-8 w-8',
		class: className = '',
		children,
		actions
	}: {
		icon?: IconComponent;
		title?: string;
		description?: string;
		iconClass?: string;
		class?: string;
		children?: Snippet;
		actions?: Snippet;
	} = $props();
</script>

<div
	class="bg-muted/20 text-muted-foreground flex flex-col items-center justify-center rounded-lg border border-dashed p-6 text-center {className}"
>
	{#if icon}
		{@const Icon = icon}
		<Icon class="{iconClass} mb-3" />
	{/if}
	<div class="text-foreground text-sm font-medium">{title}</div>
	{#if description}
		<div class="mt-1 text-sm">{description}</div>
	{/if}
	{#if actions || children}
		<div class="mt-4 empty:hidden">
			{#if actions}
				{@render actions()}
			{:else if children}
				{@render children()}
			{/if}
		</div>
	{/if}
</div>
