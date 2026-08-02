<script lang="ts">
	import PageHeader from '$lib/components/page-header.svelte';
	import PageSwitcher from '$lib/components/page-switcher.svelte';

	let { children } = $props();

	const switcherItems = [
		{ href: '/monitoring', label: 'Overview' },
		{ href: '/monitoring/clusters', label: 'Clusters' },
		{ href: '/monitoring/nodes', label: 'Nodes' },
		{ href: '/monitoring/trends', label: 'Trends' },
		{ href: '/alerts', label: 'Alerts' }
	];
</script>

<PageHeader title="Monitoring" description="Fleet-wide metrics, trends, and resource breakdown" />
<PageSwitcher items={switcherItems} />
{@render children()}
