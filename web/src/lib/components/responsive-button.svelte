<script lang="ts">
	import { Button } from '$lib/components/ui/button';

	export let icon: any; // eslint-disable-line @typescript-eslint/no-explicit-any -- Lucide图标组件类型
	export let text: string;
	export let hideTextOnMobile: boolean = true;
	export let showTextFrom: 'sm' | 'md' | 'lg' = 'sm';
	export let size: any = 'default'; // eslint-disable-line @typescript-eslint/no-explicit-any -- Button组件size属性
	export let variant: any = 'default'; // eslint-disable-line @typescript-eslint/no-explicit-any -- Button组件variant属性
	export let onclick: (() => void) | undefined = undefined;
	export let title: string = '';

	let className: string = '';
	export { className as class };
</script>

<Button
	{size}
	{variant}
	{onclick}
	{title}
	class="flex items-center gap-2 {hideTextOnMobile ? 'sm:gap-2' : 'gap-2'} {className}"
>
	<svelte:component this={icon} class="h-4 w-4" />
	{#if hideTextOnMobile}
		{#if showTextFrom === 'md'}
			<span class="hidden md:inline">{text}</span>
		{:else if showTextFrom === 'lg'}
			<span class="hidden lg:inline">{text}</span>
		{:else}
			<span class="hidden sm:inline">{text}</span>
		{/if}
	{:else}
		<span>{text}</span>
	{/if}
</Button>
