<script lang="ts">
	import { onMount } from 'svelte';
	import { api, type Cluster, type DashboardStats, type Operation } from '$lib/api';
	import PageHeader from '$lib/components/page-header.svelte';
	import StatusBadge from '$lib/components/status-badge.svelte';
	import EmptyState from '$lib/components/empty-state.svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import * as Card from '$lib/components/ui/card/index.js';
	import { Skeleton } from '$lib/components/ui/skeleton/index.js';
	import * as Alert from '$lib/components/ui/alert/index.js';
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import {
		DatabaseIcon,
		CheckmarkCircle02Icon,
		Alert02Icon,
		Activity01Icon,
		PlusSignIcon
	} from '@hugeicons/core-free-icons';

	let stats = $state<DashboardStats | null>(null);
	let clusters = $state<Cluster[]>([]);
	let ops = $state<Operation[]>([]);
	let error = $state('');
	let loading = $state(true);

	onMount(async () => {
		try {
			[stats, clusters, ops] = await Promise.all([
				api<DashboardStats>('/api/dashboard'),
				api<Cluster[]>('/api/clusters'),
				api<Operation[]>('/api/operations')
			]);
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to load dashboard';
		} finally {
			loading = false;
		}
	});

	const cards = $derived(
		stats
			? [
					{
						label: 'Clusters',
						value: stats.cluster_count,
						icon: DatabaseIcon,
						tone: 'text-primary'
					},
					{
						label: 'Healthy',
						value: stats.healthy_count,
						icon: CheckmarkCircle02Icon,
						tone: 'text-emerald-400'
					},
					{
						label: 'Degraded / warning',
						value: stats.degraded_count,
						icon: Alert02Icon,
						tone: 'text-amber-400'
					},
					{
						label: 'Databases',
						value: stats.database_count,
						icon: DatabaseIcon,
						tone: 'text-sky-400'
					},
					{
						label: 'Active operations',
						value: stats.active_operations,
						icon: Activity01Icon,
						tone: 'text-violet-400'
					},
					{
						label: 'Failed operations',
						value: stats.failed_operations,
						icon: Alert02Icon,
						tone: 'text-rose-400'
					}
				]
			: []
	);
</script>

<PageHeader title="Dashboard" description="Cluster health, backups, and active operations">
	{#snippet actions()}
		<Button href="/clusters/new">
			<HugeiconsIcon icon={PlusSignIcon} class="size-4" strokeWidth={2} />
			New cluster
		</Button>
	{/snippet}
</PageHeader>

{#if error}
	<Alert.Root variant="destructive" class="mb-6">
		<HugeiconsIcon icon={Alert02Icon} class="size-4" strokeWidth={2} />
		<Alert.Title>Could not load dashboard</Alert.Title>
		<Alert.Description>{error}</Alert.Description>
	</Alert.Root>
{/if}

{#if loading}
	<div class="mb-8 grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
		{#each Array(6) as _}
			<Skeleton class="h-28 rounded-xl" />
		{/each}
	</div>
{:else}
	<div class="mb-8 grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
		{#each cards as c}
			<Card.Root class="overflow-hidden border-border/60">
				<Card.Content class="flex items-start justify-between pt-6">
					<div>
						<p class="text-xs font-medium tracking-wide text-muted-foreground uppercase">{c.label}</p>
						<p class="mt-2 text-3xl font-semibold tabular-nums tracking-tight">{c.value}</p>
					</div>
					<div class="rounded-lg bg-muted/50 p-2.5 {c.tone}">
						<HugeiconsIcon icon={c.icon} class="size-5" strokeWidth={2} />
					</div>
				</Card.Content>
			</Card.Root>
		{/each}
	</div>
{/if}

<div class="grid gap-6 xl:grid-cols-2">
	<Card.Root class="border-border/60">
		<Card.Header class="flex-row items-center justify-between space-y-0">
			<div>
				<Card.Title>Clusters</Card.Title>
				<Card.Description>Managed PostgreSQL instances</Card.Description>
			</div>
			<Button variant="outline" size="sm" href="/clusters">View all</Button>
		</Card.Header>
		<Card.Content class="space-y-2">
			{#each clusters.slice(0, 6) as c}
				<a
					class="flex items-center justify-between rounded-lg border border-border/50 bg-muted/20 px-3 py-2.5 transition hover:bg-muted/40"
					href={`/clusters/${c.id}`}
				>
					<div class="min-w-0">
						<p class="truncate font-medium">{c.name}</p>
						<p class="text-xs text-muted-foreground">
							PostgreSQL {c.postgres_version} · {c.slug}
						</p>
					</div>
					<StatusBadge status={c.status} />
				</a>
			{:else}
				{#if !loading}
					<EmptyState
						title="No clusters yet"
						description="Provision your first PostgreSQL cluster to get started."
					>
						{#snippet action()}
							<Button size="sm" href="/clusters/new">Create cluster</Button>
						{/snippet}
					</EmptyState>
				{/if}
			{/each}
		</Card.Content>
	</Card.Root>

	<Card.Root class="border-border/60">
		<Card.Header class="flex-row items-center justify-between space-y-0">
			<div>
				<Card.Title>Recent operations</Card.Title>
				<Card.Description>Background jobs and provisioning</Card.Description>
			</div>
			<Button variant="outline" size="sm" href="/operations">View all</Button>
		</Card.Header>
		<Card.Content class="space-y-2">
			{#each ops.slice(0, 8) as op}
				<a
					class="flex items-center justify-between rounded-lg border border-border/50 bg-muted/20 px-3 py-2.5 transition hover:bg-muted/40"
					href="/operations"
				>
					<div class="min-w-0">
						<p class="font-medium font-mono text-sm">{op.job_type}</p>
						<p class="text-xs text-muted-foreground">
							{op.id.slice(0, 8)}… · {op.progress}%
						</p>
					</div>
					<StatusBadge status={op.status} />
				</a>
			{:else}
				{#if !loading}
					<p class="py-8 text-center text-sm text-muted-foreground">No operations yet</p>
				{/if}
			{/each}
		</Card.Content>
	</Card.Root>
</div>
