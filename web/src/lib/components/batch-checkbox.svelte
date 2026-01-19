<script lang="ts">
	export let checked = false;
	export let disabled = false;
	export let onclick: ((event: MouseEvent) => void) | undefined = undefined;

	let className = '';
	export { className as class };
</script>

<div class="flex-shrink-0 pt-1">
	<input
		type="checkbox"
		{checked}
		{disabled}
		{onclick}
		class="h-4 w-4 rounded border-gray-300 text-blue-600 focus:ring-blue-500 {className}"
	/>
</div>
