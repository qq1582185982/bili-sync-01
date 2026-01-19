<script lang="ts">
	let {
		src = '',
		alt = '',
		placeholder = '无图片',
		loading = 'lazy',
		decoding = 'async',
		class: className = '',
		placeholderClass = ''
	}: {
		src?: string;
		alt?: string;
		placeholder?: string;
		loading?: 'eager' | 'lazy';
		decoding?: 'sync' | 'async' | 'auto';
		class?: string;
		placeholderClass?: string;
	} = $props();

	let hasError = $state(false);

	function normalizeImageUrl(url: string): string {
		if (!url) return '';

		if (url.startsWith('https://')) return url;
		if (url.startsWith('//')) return 'https:' + url;
		if (url.startsWith('http://')) return url.replace('http://', 'https://');
		if (!url.startsWith('http')) return 'https://' + url;

		return url;
	}

	let imageUrl = $derived(normalizeImageUrl(src));

	$effect(() => {
		src;
		hasError = false;
	});
</script>

{#if imageUrl && !hasError}
	<img
		src={imageUrl}
		{alt}
		class={className}
		{loading}
		{decoding}
		crossorigin="anonymous"
		referrerpolicy="no-referrer"
		onerror={() => (hasError = true)}
	/>
{:else}
	<div
		class="bg-muted text-muted-foreground flex items-center justify-center text-xs {placeholderClass} {className}"
	>
		{placeholder}
	</div>
{/if}
