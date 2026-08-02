<script lang="ts">
	import { api, type MonitoringOverview, formatNodeCapacity } from '$lib/api';
	import StatusBadge from '$lib/components/status-badge.svelte';
	import EmptyState from '$lib/components/empty-state.svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import * as Card from '$lib/components/ui/card/index.js';
	import * as Table from '$lib/components/ui/table/index.js';
	import * as Alert from '$lib/components/ui/alert/index.js';
	import * as Tabs from '$lib/components/ui/tabs/index.js';
	import { Skeleton } from '$lib/components/ui/skeleton/index.js';
	import { Badge } from '$lib/components/ui/badge/index.js';
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import { Alert02Icon, CloudServerIcon, RefreshIcon } from '@hugeicons/core-free-icons';

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

	const rangeLabel = $derived(hours === '168' ? '7 days' : '24 hours');
	const nodes = $derived(overview?.nodes ?? []);

	const maxNodeCpu = $derived(
		nodes.length
			? Math.max(
					...nodes.map((n) => n.avg_cpu ?? n.cpu_percent ?? 0),
					1
				)
			: 1
	);
</script>

<div class="mb-6 flex flex-wrap items-center justify-between gap-3">
	<Badge variant="outline" class="text-xs">Range: {rangeLabel}</Badge>
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
	{#if nodes.length === 0}
		<EmptyState
			title="No nodes registered"
			description="Node metrics appear when Docker hosts are connected to the panel."
		/>
	{:else}
		<Card.Root class="page-card mb-6">
			<Card.Header>
				<Card.Title class="flex items-center gap-2">
					<HugeiconsIcon icon={CloudServerIcon} class="size-4" strokeWidth={2} />
					Average CPU by node
				</Card.Title>
				<Card.Description>
					Load across Docker hosts in the last {rangeLabel}
				</Card.Description>
			</Card.Header>
			<Card.Content>
				<div class="space-y-3">
					{#each nodes as node (node.node_id)}
						{@const cpu = node.avg_cpu ?? node.cpu_percent ?? 0}
						<div class="flex items-center gap-3">
							<span class="w-32 shrink-0 truncate text-sm font-medium" title={node.name}>
								{node.name}
							</span>
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
										width={(cpu / maxNodeCpu) * 100}
										height="16"
										rx="3"
										class="fill-sky-500/80"
									/>
								</svg>
							</div>
							<span class="w-14 shrink-0 text-right text-sm tabular-nums">
								{cpu > 0 ? cpu.toFixed(1) : '—'}%
							</span>
						</div>
					{/each}
				</div>
			</Card.Content>
		</Card.Root>

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
							{#each nodes as node (node.node_id)}
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
