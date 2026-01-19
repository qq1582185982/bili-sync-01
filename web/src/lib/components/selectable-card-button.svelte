<script lang="ts">
	import type { Snippet } from 'svelte';

	let {
		disabled = false,
		selected = false,
		baseClass = 'hover:bg-muted rounded-lg border text-left transition-colors',
		selectedClass = 'bg-blue-50 ring-2 ring-blue-500 dark:bg-blue-950',
		disabledClass = 'cursor-not-allowed opacity-60',
		onclick,
		class: className = '',
		children
	}: {
		disabled?: boolean;
		selected?: boolean;
		baseClass?: string;
		selectedClass?: string;
		disabledClass?: string;
		onclick?: (event: MouseEvent) => void;
		class?: string;
		children?: Snippet;
	} = $props();
</script>

<button
	type="button"
	{disabled}
	{onclick}
	class="{baseClass} {className} {disabled ? disabledClass : ''} {selected ? selectedClass : ''}"
>
	{#if children}
		{@render children()}
	{/if}
</button>
