<script lang="ts">
	import type { Snippet } from 'svelte';

	let {
		isMobile = false,
		title = '',
		subtitle = '',
		maxHeightClass = 'max-h-[calc(100vh-200px)]',
		sticky = true,
		fillHeight = true,
		headerClass = 'bg-muted',
		titleClass = 'text-foreground text-base font-medium',
		subtitleClass = 'text-muted-foreground text-sm',
		subtitleOnMobileClass = 'block',
		subtitleOnDesktopClass = 'ml-2',
		actionsClass = 'flex items-center gap-2',
		bodyClass = 'flex-1 overflow-y-auto p-3',
		footerClass = 'border-t p-3',
		showActions = true,
		showFooter = true,
		class: className = '',
		header,
		actions,
		footer,
		children
	}: {
		isMobile?: boolean;
		title?: string;
		subtitle?: string;
		maxHeightClass?: string;
		sticky?: boolean;
		fillHeight?: boolean;
		headerClass?: string;
		titleClass?: string;
		subtitleClass?: string;
		subtitleOnMobileClass?: string;
		subtitleOnDesktopClass?: string;
		actionsClass?: string;
		bodyClass?: string;
		footerClass?: string;
		showActions?: boolean;
		showFooter?: boolean;
		class?: string;
		header?: Snippet;
		actions?: Snippet;
		footer?: Snippet;
		children?: Snippet;
	} = $props();
</script>

<div
	class="bg-card flex flex-col overflow-hidden rounded-lg border {fillHeight && !isMobile
		? 'h-full'
		: ''} {sticky && !isMobile ? 'sticky top-6' : ''} {maxHeightClass} {className}"
>
	<div class="flex items-center justify-between border-b p-4 {headerClass}">
		<div class="min-w-0">
			{#if header}
				{@render header()}
			{:else}
				<span class={titleClass}>{title}</span>
				{#if subtitle}
					<span class="{subtitleClass} {isMobile ? subtitleOnMobileClass : subtitleOnDesktopClass}">
						{subtitle}
					</span>
				{/if}
			{/if}
		</div>
		{#if actions && showActions}
			<div class="{actionsClass} empty:hidden">
				{@render actions()}
			</div>
		{/if}
	</div>

	<div class={bodyClass}>
		{#if children}
			{@render children()}
		{/if}
	</div>

	{#if footer && showFooter}
		<div class={footerClass}>
			{@render footer()}
		</div>
	{/if}
</div>
