<script lang="ts">
	import { page } from '$app/stores';
	import { goto } from '$app/navigation';
	import { api, type Cluster } from '$lib/api';
	import ClusterSelect from '$lib/components/cluster-select.svelte';
	import PageSwitcher from '$lib/components/page-switcher.svelte';
	import type { Snippet } from 'svelte';

	let { children }: { children: Snippet } = $props();

	let cluster = $state<Cluster | null>(null);

	const id = $derived($page.params.id);

	const subpath = $derived.by(() => {
		const pathname = $page.url.pathname;
		const prefix = `/clusters/${id}`;
		if (pathname === prefix || pathname === `${prefix}/`) return '';
		if (pathname.startsWith(`${prefix}/`)) {
			return pathname.slice(prefix.length + 1);
		}
		return '';
	});

	const switcherItems = $derived([
		{ href: `/clusters/${id}`, label: 'Overview' },
		{ href: `/clusters/${id}/databases`, label: 'Databases & users' },
		{ href: `/clusters/${id}/browser`, label: 'Browser' },
		{ href: `/clusters/${id}/query`, label: 'SQL' },
		{ href: `/clusters/${id}/backup`, label: 'Backups' },
		{ href: `/clusters/${id}/timetravel`, label: 'Time travel' }
	]);

	function navigateToCluster(clusterId: string) {
		const base = `/clusters/${clusterId}`;
		goto(subpath ? `${base}/${subpath}` : base);
	}

	$effect(() => {
		if (!id) return;
		api<Cluster>(`/api/clusters/${id}`)
			.then((c) => {
				cluster = c;
			})
			.catch(() => {});
	});
</script>

<div class="mb-6 flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
	<div class="flex flex-wrap items-center gap-3">
		<ClusterSelect value={id} onValueChange={navigateToCluster} />
		{#if cluster}
			<span class="text-sm text-muted-foreground">{cluster.slug}</span>
		{/if}
	</div>
</div>

<PageSwitcher items={switcherItems} />

{@render children()}
