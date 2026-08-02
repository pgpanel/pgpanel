<script lang="ts">
	import { onMount } from 'svelte';
	import { api, type Node } from '$lib/api';
	import SearchableSelect, { type SearchableOption } from '$lib/components/searchable-select.svelte';

	let {
		value = $bindable(''),
		placeholder = 'Select node…',
		disabled = false,
		clearable = false,
		class: className = '',
		onValueChange
	}: {
		value?: string;
		placeholder?: string;
		disabled?: boolean;
		clearable?: boolean;
		class?: string;
		onValueChange?: (v: string) => void;
	} = $props();

	let items = $state<SearchableOption[]>([]);
	let loading = $state(true);

	onMount(() => {
		api<Node[]>('/api/nodes')
			.then((nodes) => {
				items = nodes.map((node) => ({
					value: node.id,
					label: node.name,
					description: `${node.kind} · ${node.status}`,
					keywords: [node.slug, node.kind, node.status]
				}));
			})
			.finally(() => {
				loading = false;
			});
	});
</script>

<SearchableSelect
	bind:value
	{items}
	{placeholder}
	{disabled}
	{clearable}
	{loading}
	class={className}
	{onValueChange}
/>
