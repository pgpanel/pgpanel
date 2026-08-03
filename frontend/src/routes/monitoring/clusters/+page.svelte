<script lang="ts">
	import { api, type MonitoringOverview } from '$lib/api';
	import StatusBadge from '$lib/components/status-badge.svelte';
	import EmptyState from '$lib/components/empty-state.svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import * as Card from '$lib/components/ui/card/index.js';
	import * as Table from '$lib/components/ui/table/index.js';
	import * as Alert from '$lib/components/ui/alert/index.js';
	import * as Tabs from '$lib/components/ui/tabs/index.js';
	import * as Select from '$lib/components/ui/select/index.js';
	import { Skeleton } from '$lib/components/ui/skeleton/index.js';
	import { Badge } from '$lib/components/ui/badge/index.js';
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import { Alert02Icon, Chart01Icon, RefreshIcon } from '@hugeicons/core-free-icons';

	let overview = $state<MonitoringOverview | null>(null);
	let error = $state('');
	let loading = $state(true);
	let hours = $state('24');
	let clusterFilter = $state('all');

	$effect(() => {
		if (hours) load();
	});

	async function load() {
		loading = true;
		error = '';
		try {
			overview = await api<MonitoringOverview>(`/api/monitoring/overview?hours=${hours}`);
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to load monitoring';
		} finally {
			loading = false;
		}
	}

	const rangeLabel = $derived(hours === '168' ? '7 days' : '24 hours');

	const filteredClusters = $derived(
		overview
			? clusterFilter === 'all'
				? overview.top_clusters
				: overview.top_clusters.filter((c) => c.cluster_id === clusterFilter)
			: []
	);

	const maxCpuInView = $derived(
		filteredClusters.length ? Math.max(...filteredClusters.map((c) => c.cpu_percent), 1) : 1
	);

	const clusterOptions = $derived(
		overview
			? overview.top_clusters.map((c) => ({ value: c.cluster_id, label: c.name }))
			: []
	);

	const selectedClusterLabel = $derived(
		clusterFilter === 'all'
			? 'All clusters'
			: (clusterOptions.find((o) => o.value === clusterFilter)?.label ?? 'Cluster')
	);
</script>

<div class="mb-6 flex flex-wrap items-center justify-between gap-3">
	<div class="flex flex-wrap items-center gap-2">
		{#if !loading && clusterOptions.length > 0}
			<Select.Root type="single" bind:value={clusterFilter}>
				<Select.Trigger class="w-[200px]">{selectedClusterLabel}</Select.Trigger>
				<Select.Content>
					<Select.Item value="all" label="All clusters">All clusters</Select.Item>
					{#each clusterOptions as opt, i (`${i}-${opt.value}`)}
						<Select.Item value={opt.value} label={opt.label}>{opt.label}</Select.Item>
					{/each}
				</Select.Content>
			</Select.Root>
		{/if}
		<Badge variant="outline" class="text-xs">Range: {rangeLabel}</Badge>
	</div>
	<div class="flex flex-wrap items-center gap-2">
		<Tabs.Root bind:value={hours}>
			<Tabs.List class="h-9">
				<Tabs.Trigger value="24" class="px-3 text-xs">24h</Tabs.Trigger>
				<Tabs.Trigger value="168" class="px-3 text-xs">7d</Tabs.Trigger>
			</Tabs.List>
		</Tabs.Root>
		<Button variant="outline" size="sm" onclick={() => load()} disabled={loading}>
			<HugeiconsIcon icon={RefreshIcon} class="size-4" strokeWidth={2} />
			Refresh
		</Button>
	</div>
</div>

{#if error}
	<Alert.Root variant="destructive" class="mb-4">
		<HugeiconsIcon icon={Alert02Icon} class="size-4" strokeWidth={2} />
		<Alert.Description>{error}</Alert.Description>
	</Alert.Root>
{/if}

{#if loading && !overview}
	<Skeleton class="mb-6 h-48 rounded-xl" />
	<Skeleton class="h-64 rounded-xl" />
{:else if overview}
	{#if overview.top_clusters.length === 0}
		<EmptyState
			title="No cluster metrics yet"
			description="CPU and memory samples appear after the metrics scheduler collects data from your clusters."
		/>
	{:else}
		<Card.Root class="page-card mb-6">
			<Card.Header>
				<Card.Title class="flex items-center gap-2">
					<HugeiconsIcon icon={Chart01Icon} class="size-4" strokeWidth={2} />
					CPU load by cluster
				</Card.Title>
				<Card.Description>Top clusters in the last {rangeLabel} (sparse metrics may show few bars)</Card.Description>
			</Card.Header>
			<Card.Content>
				{#if filteredClusters.length === 0}
					<p class="text-sm text-muted-foreground">No data for the selected cluster.</p>
				{:else}
					<div class="space-y-3">
						{#each filteredClusters as row, i (`${i}-${row.cluster_id}`)}
							<div class="flex items-center gap-3">
								<a
									href={`/clusters/${row.cluster_id}`}
									class="w-32 shrink-0 truncate text-sm font-medium hover:underline"
									title={row.name}
								>
									{row.name}
								</a>
								<div class="relative h-6 flex-1 overflow-hidden rounded-md bg-muted/50">
									<svg
										class="h-full w-full"
										viewBox="0 0 100 24"
										preserveAspectRatio="none"
										aria-hidden="true"
									>
										<rect
											x="0"
											y="4"
											width={(row.cpu_percent / maxCpuInView) * 100}
											height="16"
											rx="3"
											class="fill-primary/80"
										/>
									</svg>
								</div>
								<span class="w-14 shrink-0 text-right text-sm tabular-nums">
									{row.cpu_percent.toFixed(1)}%
								</span>
							</div>
						{/each}
					</div>
				{/if}
			</Card.Content>
		</Card.Root>

		<Card.Root class="page-card">
			<Card.Header>
				<Card.Title>Cluster metrics</Card.Title>
				<Card.Description>Latest samples per cluster in range</Card.Description>
			</Card.Header>
			<Card.Content class="p-0">
				<div class="table-scroll">
					<Table.Root>
						<Table.Header>
							<Table.Row class="hover:bg-transparent">
								<Table.Head>Cluster</Table.Head>
								<Table.Head>Status</Table.Head>
								<Table.Head class="text-right">CPU %</Table.Head>
								<Table.Head class="text-right">Load bar</Table.Head>
								<Table.Head class="text-right">Memory</Table.Head>
							</Table.Row>
						</Table.Header>
						<Table.Body>
							{#each filteredClusters as row, i (`${i}-${row.cluster_id}`)}
								<Table.Row>
									<Table.Cell>
										<a
											href={`/clusters/${row.cluster_id}`}
											class="font-medium hover:underline"
										>
											{row.name}
										</a>
									</Table.Cell>
									<Table.Cell>
										<StatusBadge status={row.status} />
									</Table.Cell>
									<Table.Cell class="text-right tabular-nums">
										{row.cpu_percent.toFixed(1)}
									</Table.Cell>
									<Table.Cell class="text-right">
										<div class="ms-auto h-2 w-20 overflow-hidden rounded-full bg-muted">
											<div
												class="h-full rounded-full bg-primary"
												style={`width: ${Math.min(100, row.cpu_percent)}%`}
											></div>
										</div>
									</Table.Cell>
									<Table.Cell class="text-right tabular-nums text-xs">
										{Math.round(row.memory_usage_mb)} MB
									</Table.Cell>
								</Table.Row>
							{/each}
						</Table.Body>
					</Table.Root>
				</div>
			</Card.Content>
		</Card.Root>
	{/if}
{/if}
