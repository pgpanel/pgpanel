<script lang="ts">
	import { api, type MonitoringOverview, formatNodeCapacity } from '$lib/api';
	import PageHeader from '$lib/components/page-header.svelte';
	import StatusBadge from '$lib/components/status-badge.svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import * as Card from '$lib/components/ui/card/index.js';
	import * as Table from '$lib/components/ui/table/index.js';
	import * as Alert from '$lib/components/ui/alert/index.js';
	import * as Tabs from '$lib/components/ui/tabs/index.js';
	import { Skeleton } from '$lib/components/ui/skeleton/index.js';
	import { Badge } from '$lib/components/ui/badge/index.js';
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import {
		Alert02Icon,
		Chart01Icon,
		DatabaseIcon,
		CheckmarkCircle02Icon,
		CloudBackupIcon,
		DatabaseSyncIcon,
		RefreshIcon,
		CloudServerIcon,
		Activity01Icon
	} from '@hugeicons/core-free-icons';

	let overview = $state<MonitoringOverview | null>(null);
	let error = $state('');
	let loading = $state(true);
	let hours = $state('24');

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

	const backupSuccessRate = $derived.by(() => {
		if (!overview) return null;
		if (overview.backup_success_rate != null) return overview.backup_success_rate * 100;
		const total = overview.backups_last_24h + overview.failed_backups_24h;
		if (total === 0) return null;
		return (overview.backups_last_24h / total) * 100;
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
						label: 'Databases',
						value: overview.databases_total ?? '—',
						icon: DatabaseIcon,
						tone: 'text-sky-400'
					},
					{
						label: 'Nodes online',
						value:
							overview.online_nodes != null && overview.node_count != null
								? `${overview.online_nodes}/${overview.node_count}`
								: (overview.node_count ?? '—'),
						icon: CloudServerIcon,
						tone: 'text-primary'
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
						tone: 'text-emerald-400'
					},
					{
						label: 'Max CPU %',
						value: overview.max_cpu_24h != null ? overview.max_cpu_24h.toFixed(1) : '—',
						icon: Chart01Icon,
						tone: 'text-rose-400'
					},
					{
						label: 'Avg memory MB',
						value: Math.round(overview.avg_memory_mb),
						icon: Chart01Icon,
						tone: 'text-sky-400'
					},
					{
						label: 'Ops failed',
						value: overview.operations_failed_24h ?? '—',
						icon: Activity01Icon,
						tone: 'text-rose-400'
					},
					{
						label: 'Backups',
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
						label: 'Backup success',
						value: backupSuccessRate != null ? `${backupSuccessRate.toFixed(0)}%` : '—',
						icon: CheckmarkCircle02Icon,
						tone: 'text-emerald-400'
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
		overview?.series.length ? Math.max(...overview.series.map((p) => p.cpu), 1) : 1
	);
	const maxMem = $derived(
		overview?.series.length ? Math.max(...overview.series.map((p) => p.memory_mb), 1) : 1
	);

	const rangeLabel = $derived(hours === '168' ? '7 days' : '24 hours');
</script>

<PageHeader title="Monitoring" description="Fleet-wide metrics, trends, and top clusters">
	{#snippet actions()}
		<Tabs.Root bind:value={hours}>
			<Tabs.List class="h-9">
				<Tabs.Trigger value="24" class="px-3 text-xs">24h</Tabs.Trigger>
				<Tabs.Trigger value="168" class="px-3 text-xs">7d</Tabs.Trigger>
			</Tabs.List>
		</Tabs.Root>
		<Button variant="outline" size="sm" href="/alerts">Alerts</Button>
		<Button variant="outline" size="sm" onclick={() => load()} disabled={loading}>
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

{#if loading && !overview}
	<div class="mb-8 grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
		{#each Array(12) as _}
			<Skeleton class="h-24 rounded-xl" />
		{/each}
	</div>
{:else if overview}
	<div class="mb-2 flex items-center gap-2">
		<Badge variant="outline" class="text-xs">Range: {rangeLabel}</Badge>
	</div>

	<div class="mb-8 grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
		{#each kpis as k}
			<Card.Root class="page-card kpi-tile">
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
		<Card.Root class="page-card">
			<Card.Header>
				<Card.Title class="flex items-center gap-2">
					<HugeiconsIcon icon={Chart01Icon} class="size-4" strokeWidth={2} />
					CPU & memory trend
				</Card.Title>
				<Card.Description>Fleet averages over the last {rangeLabel}</Card.Description>
			</Card.Header>
			<Card.Content>
				{#if overview.series.length === 0}
					<p class="text-sm text-muted-foreground">No metric samples yet.</p>
				{:else}
					<div class="mb-3 flex gap-4 text-xs text-muted-foreground">
						<span class="flex items-center gap-1.5">
							<span class="size-2 rounded-sm bg-primary"></span> CPU %
						</span>
						<span class="flex items-center gap-1.5">
							<span class="size-2 rounded-sm bg-chart-2"></span> Memory MB
						</span>
					</div>
					<div class="flex h-36 items-end gap-0.5">
						{#each overview.series as point (point.at)}
							<div class="group relative flex min-w-0 flex-1 flex-col items-stretch justify-end gap-0.5">
								<div
									class="rounded-t bg-chart-2/60 transition-colors group-hover:bg-chart-2"
									style={`height: ${Math.max(2, (point.memory_mb / maxMem) * 45)}%`}
								></div>
								<div
									class="rounded-t bg-primary/70 transition-colors group-hover:bg-primary"
									style={`height: ${Math.max(4, (point.cpu / maxCpu) * 55)}%`}
									title={`${point.at}: ${point.cpu.toFixed(1)}% CPU, ${Math.round(point.memory_mb)} MB`}
								></div>
							</div>
						{/each}
					</div>
					<div class="table-scroll mt-4 max-h-48">
						<Table.Root>
							<Table.Header>
								<Table.Row class="hover:bg-transparent">
									<Table.Head>Time</Table.Head>
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
					</div>
				{/if}
			</Card.Content>
		</Card.Root>

		<Card.Root class="page-card">
			<Card.Header>
				<Card.Title>Top clusters by CPU</Card.Title>
				<Card.Description>Highest load in the selected range</Card.Description>
			</Card.Header>
			<Card.Content class="p-0">
				{#if overview.top_clusters.length === 0}
					<p class="p-6 text-sm text-muted-foreground">No cluster metrics collected yet.</p>
				{:else}
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
				{/if}
			</Card.Content>
		</Card.Root>
	</div>

	{#if overview.nodes && overview.nodes.length > 0}
		<Card.Root class="page-card">
			<Card.Header>
				<Card.Title>Per-node breakdown</Card.Title>
				<Card.Description>Capacity and load by Docker host</Card.Description>
			</Card.Header>
			<Card.Content class="p-0">
				<div class="table-scroll">
					<Table.Root>
						<Table.Header>
							<Table.Row class="hover:bg-transparent">
								<Table.Head>Node</Table.Head>
								<Table.Head>Status</Table.Head>
								<Table.Head>Capacity</Table.Head>
								<Table.Head class="text-right">CPU %</Table.Head>
								<Table.Head class="text-right">Memory MB</Table.Head>
							</Table.Row>
						</Table.Header>
						<Table.Body>
							{#each overview.nodes as node (node.node_id)}
								<Table.Row>
									<Table.Cell class="font-medium">{node.name}</Table.Cell>
									<Table.Cell><StatusBadge status={node.status} /></Table.Cell>
									<Table.Cell class="tabular-nums text-sm">
										{formatNodeCapacity(node.cluster_count, node.max_clusters)}
									</Table.Cell>
									<Table.Cell class="text-right tabular-nums">
										{node.avg_cpu != null
											? node.avg_cpu.toFixed(1)
											: node.cpu_percent != null
												? node.cpu_percent.toFixed(1)
												: '—'}
									</Table.Cell>
									<Table.Cell class="text-right tabular-nums">
										{node.memory_mb != null ? Math.round(node.memory_mb) : '—'}
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
