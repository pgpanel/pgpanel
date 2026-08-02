<script lang="ts">
	import { onMount } from 'svelte';
	import { api, type MonitoringOverview } from '$lib/api';
	import PageHeader from '$lib/components/page-header.svelte';
	import StatusBadge from '$lib/components/status-badge.svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import * as Card from '$lib/components/ui/card/index.js';
	import * as Table from '$lib/components/ui/table/index.js';
	import * as Alert from '$lib/components/ui/alert/index.js';
	import { Skeleton } from '$lib/components/ui/skeleton/index.js';
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import {
		Alert02Icon,
		Chart01Icon,
		DatabaseIcon,
		CheckmarkCircle02Icon,
		CloudBackupIcon,
		DatabaseSyncIcon,
		RefreshIcon
	} from '@hugeicons/core-free-icons';

	let overview = $state<MonitoringOverview | null>(null);
	let error = $state('');
	let loading = $state(true);

	async function load() {
		overview = await api<MonitoringOverview>('/api/monitoring/overview');
	}

	onMount(() => {
		load()
			.catch((e) => (error = e instanceof Error ? e.message : 'Failed to load monitoring'))
			.finally(() => (loading = false));
	});

	const kpis = $derived(
		overview
			? [
					{ label: 'Clusters', value: overview.cluster_count, icon: DatabaseIcon, tone: 'text-primary' },
					{
						label: 'Healthy',
						value: overview.healthy_count,
						icon: CheckmarkCircle02Icon,
						tone: 'text-emerald-400'
					},
					{
						label: 'Open alerts',
						value: overview.open_alerts,
						icon: Alert02Icon,
						tone: 'text-amber-400'
					},
					{
						label: 'Avg CPU %',
						value: overview.avg_cpu.toFixed(1),
						icon: Chart01Icon,
						tone: 'text-violet-400'
					},
					{
						label: 'Avg memory MB',
						value: Math.round(overview.avg_memory_mb),
						icon: Chart01Icon,
						tone: 'text-sky-400'
					},
					{
						label: 'Backups (24h)',
						value: overview.backups_last_24h,
						icon: CloudBackupIcon,
						tone: 'text-emerald-400'
					},
					{
						label: 'Failed backups',
						value: overview.failed_backups_24h,
						icon: Alert02Icon,
						tone: 'text-rose-400'
					},
					{
						label: 'Replicas healthy',
						value: `${overview.replica_healthy}/${overview.replica_total}`,
						icon: DatabaseSyncIcon,
						tone: 'text-primary'
					}
				]
			: []
	);

	const maxCpu = $derived(
		overview?.series.length
			? Math.max(...overview.series.map((p) => p.cpu), 1)
			: 1
	);
</script>

<PageHeader title="Monitoring" description="Fleet-wide metrics, trends, and top clusters (24h)">
	{#snippet actions()}
		<Button variant="outline" size="sm" href="/alerts">View alerts</Button>
		<Button variant="outline" size="sm" onclick={() => load()}>
			<HugeiconsIcon icon={RefreshIcon} class="size-4" strokeWidth={2} />
			Refresh
		</Button>
	{/snippet}
</PageHeader>

{#if error}
	<Alert.Root variant="destructive" class="mb-4">
		<HugeiconsIcon icon={Alert02Icon} class="size-4" strokeWidth={2} />
		<Alert.Description>{error}</Alert.Description>
	</Alert.Root>
{/if}

{#if loading}
	<div class="mb-8 grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
		{#each Array(8) as _}
			<Skeleton class="h-24 rounded-xl" />
		{/each}
	</div>
{:else if overview}
	<div class="mb-8 grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
		{#each kpis as k}
			<Card.Root class="border-border/60">
				<Card.Content class="flex items-start justify-between pt-6">
					<div>
						<p class="text-xs font-medium tracking-wide text-muted-foreground uppercase">{k.label}</p>
						<p class="mt-2 text-2xl font-semibold tabular-nums tracking-tight">{k.value}</p>
					</div>
					<div class="rounded-lg bg-muted/50 p-2.5 {k.tone}">
						<HugeiconsIcon icon={k.icon} class="size-5" strokeWidth={2} />
					</div>
				</Card.Content>
			</Card.Root>
		{/each}
	</div>

	<div class="mb-6 grid gap-6 xl:grid-cols-2">
		<Card.Root class="border-border/60">
			<Card.Header>
				<Card.Title class="flex items-center gap-2">
					<HugeiconsIcon icon={Chart01Icon} class="size-4" strokeWidth={2} />
					CPU trend (hourly)
				</Card.Title>
				<Card.Description>Average fleet CPU % over the last 24 hours</Card.Description>
			</Card.Header>
			<Card.Content>
				{#if overview.series.length === 0}
					<p class="text-sm text-muted-foreground">No metric samples yet.</p>
				{:else}
					<div class="flex h-32 items-end gap-0.5">
						{#each overview.series as point (point.at)}
							<div
								class="min-w-0 flex-1 rounded-t bg-primary/70 transition-colors hover:bg-primary"
								style={`height: ${Math.max(4, (point.cpu / maxCpu) * 100)}%`}
								title={`${point.at}: ${point.cpu.toFixed(1)}% CPU`}
							></div>
						{/each}
					</div>
					<Table.Root class="mt-4">
						<Table.Header>
							<Table.Row class="hover:bg-transparent">
								<Table.Head>Hour</Table.Head>
								<Table.Head class="text-right">CPU %</Table.Head>
								<Table.Head class="text-right">Memory MB</Table.Head>
							</Table.Row>
						</Table.Header>
						<Table.Body>
							{#each overview.series.slice(-12) as point (point.at)}
								<Table.Row>
									<Table.Cell class="font-mono text-xs">{point.at}</Table.Cell>
									<Table.Cell class="text-right tabular-nums text-xs"
										>{point.cpu.toFixed(1)}</Table.Cell
									>
									<Table.Cell class="text-right tabular-nums text-xs"
										>{Math.round(point.memory_mb)}</Table.Cell
									>
								</Table.Row>
							{/each}
						</Table.Body>
					</Table.Root>
				{/if}
			</Card.Content>
		</Card.Root>

		<Card.Root class="border-border/60">
			<Card.Header>
				<Card.Title>Top clusters by CPU</Card.Title>
				<Card.Description>Highest load in the last 24 hours</Card.Description>
			</Card.Header>
			<Card.Content class="p-0">
				{#if overview.top_clusters.length === 0}
					<p class="p-6 text-sm text-muted-foreground">No cluster metrics collected yet.</p>
				{:else}
					<Table.Root>
						<Table.Header>
							<Table.Row class="hover:bg-transparent">
								<Table.Head>Cluster</Table.Head>
								<Table.Head>Status</Table.Head>
								<Table.Head class="text-right">CPU %</Table.Head>
								<Table.Head class="text-right">Memory</Table.Head>
							</Table.Row>
						</Table.Header>
						<Table.Body>
							{#each overview.top_clusters as row (row.cluster_id)}
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
									<Table.Cell class="text-right tabular-nums"
										>{row.cpu_percent.toFixed(1)}</Table.Cell
									>
									<Table.Cell class="text-right tabular-nums text-xs">
										{Math.round(row.memory_usage_mb)} MB
									</Table.Cell>
								</Table.Row>
							{/each}
						</Table.Body>
					</Table.Root>
				{/if}
			</Card.Content>
		</Card.Root>
	</div>
{/if}
