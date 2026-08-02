<script lang="ts">
	import { api, type DatabaseRecord } from '$lib/api';
	import SearchableSelect, { type SearchableOption } from '$lib/components/searchable-select.svelte';

	let {
		clusterId,
		value = $bindable(''),
		includePostgres = true,
		placeholder = 'Select database…',
		disabled = false,
		clearable = false,
		class: className = '',
		id,
		onValueChange
	}: {
		clusterId: string;
		value?: string;
		includePostgres?: boolean;
		placeholder?: string;
		disabled?: boolean;
		clearable?: boolean;
		class?: string;
		id?: string;
		onValueChange?: (v: string) => void;
	} = $props();

	let items = $state<SearchableOption[]>([]);
	let loading = $state(false);

	async function loadDatabases(id: string) {
		if (!id) {
			items = [];
			return;
		}
		loading = true;
		try {
			const databases = await api<DatabaseRecord[]>(`/api/clusters/${id}/databases`);
			const options = databases.map((database) => ({
				value: database.name,
				label: database.name
			}));
			if (includePostgres && !options.some((option) => option.value === 'postgres')) {
				options.unshift({ value: 'postgres', label: 'postgres' });
			}
			items = options;
		} catch {
			items =
				includePostgres ? [{ value: 'postgres', label: 'postgres' }] : [];
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		loadDatabases(clusterId);
	});
</script>

<SearchableSelect
	bind:value
	{items}
	{placeholder}
	{disabled}
	{clearable}
	{loading}
	mono
	class={className}
	{id}
	{onValueChange}
/>
