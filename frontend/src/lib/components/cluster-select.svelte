<script lang="ts">
	import { onMount } from 'svelte';
	import { api, type Cluster } from '$lib/api';
	import SearchableSelect, { type SearchableOption } from '$lib/components/searchable-select.svelte';

	let {
		value = $bindable(''),
		placeholder = 'Select cluster…',
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
		api<Cluster[]>('/api/clusters')
			.then((clusters) => {
				items = clusters.map((cluster) => ({
					value: cluster.id,
					label: cluster.name,
					description: cluster.slug,
					keywords: [cluster.slug, cluster.status, cluster.health]
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
