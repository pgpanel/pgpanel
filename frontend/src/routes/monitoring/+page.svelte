<script lang="ts">
	import { api, type MonitoringOverview } from '$lib/api';
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
		CloudServerIcon,
		DatabaseSyncIcon,
		RefreshIcon,
		ArrowRight01Icon
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
					{
						label: 'Clusters',
						value: overview.cluster_count,
						icon: DatabaseIcon,
						tone: 'text-primary'
					},
					{
						label: 'Healthy',
						value: overview.healthy_count,
						icon: CheckmarkCircle02Icon,
						tone: 'text-emerald-400'
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
						label: 'Avg memory MB',
						value: Math.round(overview.avg_memory_mb),
						icon: Chart01Icon,
						tone: 'text-sky-400'
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

	const rangeLabel = $derived(hours === '168' ? '7 days' : '24 hours');

	const quickLinks = [
		{
			href: '/monitoring/clusters',
			title: 'Cluster load',
			description: 'CPU and memory by cluster with bar charts'
		},
		{
			href: '/monitoring/nodes',
			title: 'Node breakdown',
			description: 'Per-host capacity and utilization'
		},
		{
			href: '/monitoring/trends',
			title: 'Trend charts',
			description: 'Fleet CPU and memory over time'
		},
		{ href: '/alerts', title: 'Alerts', description: 'Open and recent alert history' }
	];
</script>

<div class="mb-6 flex flex-wrap items-center justify-end gap-2">
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

{#if error}
	<Alert.Root variant="destructive" class="mb-4">
		<HugeiconsIcon icon={Alert02Icon} class="size-4" strokeWidth={2} />
		<Alert.Description>{error}</Alert.Description>
	</Alert.Root>
{/if}

{#if loading && !overview}
	<div class="mb-8 grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
		{#each Array(8) as _}
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
						<p class="text-xs font-medium tracking-wide text-muted-foreground uppercase">
							{k.label}
						</p>
						<p class="mt-2 text-2xl font-semibold tabular-nums tracking-tight">{k.value}</p>
					</div>
					<div class="rounded-lg bg-muted/50 p-2.5 {k.tone}">
						<HugeiconsIcon icon={k.icon} class="size-5" strokeWidth={2} />
					</div>
				</Card.Content>
			</Card.Root>
		{/each}
	</div>

	<div class="mb-8 grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
		{#each quickLinks as link (link.href)}
			<a href={link.href} class="group block">
				<Card.Root class="page-card transition-colors group-hover:border-primary/40">
					<Card.Content class="flex items-start justify-between pt-6">
						<div class="space-y-1">
							<p class="font-medium">{link.title}</p>
							<p class="text-sm text-muted-foreground">{link.description}</p>
						</div>
						<HugeiconsIcon
							icon={ArrowRight01Icon}
							class="size-4 shrink-0 text-muted-foreground transition-transform group-hover:translate-x-0.5 group-hover:text-primary"
							strokeWidth={2}
						/>
					</Card.Content>
				</Card.Root>
			</a>
		{/each}
	</div>

	<Card.Root class="page-card">
		<Card.Header>
			<Card.Title>Top clusters by CPU</Card.Title>
			<Card.Description>
				Preview of highest load in the selected range —
				<a href="/monitoring/clusters" class="text-primary hover:underline">view all clusters</a>
			</Card.Description>
		</Card.Header>
		<Card.Content class="p-0">
			{#if overview.top_clusters.length === 0}
				<p class="p-6 text-sm text-muted-foreground">
					No cluster metrics collected yet. Samples appear after the metrics scheduler runs.
				</p>
			{:else}
				<div class="table-scroll">
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
							{#each overview.top_clusters.slice(0, 5) as row (row.cluster_id)}
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
{/if}
