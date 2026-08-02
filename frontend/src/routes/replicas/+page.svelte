<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/stores';
	import { api, type Cluster, type ClusterReplica, type Node, formatRelative } from '$lib/api';
	import { trackOperation } from '$lib/jobs';
	import PageHeader from '$lib/components/page-header.svelte';
	import StatusBadge from '$lib/components/status-badge.svelte';
	import EmptyState from '$lib/components/empty-state.svelte';
	import ConfirmDialog from '$lib/components/confirm-dialog.svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Input } from '$lib/components/ui/input/index.js';
	import { Label } from '$lib/components/ui/label/index.js';
	import { Switch } from '$lib/components/ui/switch/index.js';
	import * as Card from '$lib/components/ui/card/index.js';
	import * as Table from '$lib/components/ui/table/index.js';
	import * as Alert from '$lib/components/ui/alert/index.js';
	import * as Select from '$lib/components/ui/select/index.js';
	import { toast } from 'svelte-sonner';
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import {
		Alert02Icon,
		DatabaseSyncIcon,
		PlusSignIcon,
		RefreshIcon,
		Delete02Icon,
		ArrowUp01Icon
	} from '@hugeicons/core-free-icons';

	let replicas = $state<ClusterReplica[]>([]);
	let clusters = $state<Cluster[]>([]);
	let nodes = $state<Node[]>([]);
	let error = $state('');
	let loading = $state(true);
	let creating = $state(false);
	let busy = $state<string | null>(null);
	let promoteOpen = $state(false);
	let promoteTargetId = $state<string | null>(null);
	let deleteOpen = $state(false);
	let deleteTarget = $state<ClusterReplica | null>(null);

	const clusterFilter = $derived($page.url.searchParams.get('cluster') ?? '');

	let primary_cluster_id = $state('');
	let name = $state('');
	let target_node_id = $state('');
	let mode = $state('scheduled_sync');
	let sync_cron = $state('*/30 * * * *');
	let auto_failover = $state(false);
	let provision_now = $state(true);

	const filteredReplicas = $derived(
		clusterFilter
			? replicas.filter((r) => r.primary_cluster_id === clusterFilter)
			: replicas
	);

	const clusterName = $derived(
		clusters.find((c) => c.id === clusterFilter)?.name ?? clusterFilter.slice(0, 8)
	);

	async function load() {
		const [r, c, n] = await Promise.all([
			api<ClusterReplica[]>('/api/replicas'),
			api<Cluster[]>('/api/clusters'),
			api<Node[]>('/api/nodes')
		]);
		replicas = r;
		clusters = c;
		nodes = n;
		if (!primary_cluster_id && c.length > 0) {
			primary_cluster_id = clusterFilter || c[0].id;
		}
		if (!target_node_id && n.length > 0) {
			target_node_id = n.find((nd) => nd.is_default)?.id ?? n[0].id;
		}
	}

	onMount(() => {
		load()
			.catch((e) => (error = e instanceof Error ? e.message : 'Failed to load replicas'))
			.finally(() => (loading = false));
	});

	async function createReplica(e: Event) {
		e.preventDefault();
		if (!primary_cluster_id || !target_node_id) {
			toast.error('Select primary cluster and target node');
			return;
		}
		creating = true;
		try {
			const res = await api<{ operation_id?: string }>(
				`/api/clusters/${primary_cluster_id}/replicas`,
				{
					method: 'POST',
					body: JSON.stringify({
						name,
						target_node_id,
						mode,
						sync_cron,
						auto_failover,
						provision_now
					})
				}
			);
			toast.success('Replica created');
			if (res.operation_id) {
				trackOperation(res.operation_id, { title: 'Provision replica', onDone: () => load() });
			}
			name = '';
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Create failed');
		} finally {
			creating = false;
		}
	}

	async function syncReplica(id: string) {
		busy = id;
		try {
			const res = await api<{ operation_id: string }>(`/api/replicas/${id}/sync`, {
				method: 'POST'
			});
			trackOperation(res.operation_id, { title: 'Sync replica', onDone: () => load() });
			toast.success('Sync queued');
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Sync failed');
		} finally {
			busy = null;
		}
	}

	function askPromoteReplica(id: string) {
		promoteTargetId = id;
		promoteOpen = true;
	}

	async function confirmPromoteReplica() {
		if (!promoteTargetId) return;
		busy = promoteTargetId;
		try {
			const res = await api<{ operation_id: string }>(`/api/replicas/${promoteTargetId}/promote`, {
				method: 'POST'
			});
			trackOperation(res.operation_id, { title: 'Promote replica', onDone: () => load() });
			toast.success('Promotion queued');
			promoteTargetId = null;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Promote failed');
		} finally {
			busy = null;
		}
	}

	function askDeleteReplica(r: ClusterReplica) {
		deleteTarget = r;
		deleteOpen = true;
	}

	async function confirmDeleteReplica() {
		if (!deleteTarget) return;
		try {
			await api(`/api/replicas/${deleteTarget.id}`, { method: 'DELETE' });
			toast.success('Replica deleted');
			deleteTarget = null;
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Delete failed');
		}
	}

	function clusterLabel(id: string) {
		return clusters.find((c) => c.id === id)?.name ?? id.slice(0, 8);
	}

	function nodeLabel(id: string) {
		return nodes.find((n) => n.id === id)?.name ?? id.slice(0, 8);
	}
</script>

<PageHeader
	title="Replicas"
	description={clusterFilter
		? `Replicas for ${clusterName}`
		: 'Cross-node cluster replication and scheduled sync'}
>
	{#snippet actions()}
		{#if clusterFilter}
			<Button variant="outline" size="sm" href="/replicas">Show all</Button>
		{/if}
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

<div class="mb-6 grid gap-6 lg:grid-cols-2">
	<Card.Root class="border-border/60">
		<Card.Header>
			<Card.Title class="flex items-center gap-2">
				<HugeiconsIcon icon={DatabaseSyncIcon} class="size-5 text-primary" strokeWidth={2} />
				Create replica
			</Card.Title>
			<Card.Description>Duplicate a primary cluster to a target node</Card.Description>
		</Card.Header>
		<Card.Content>
			<form class="space-y-4" onsubmit={createReplica}>
				<div class="space-y-2">
					<Label>Primary cluster</Label>
					<Select.Root type="single" bind:value={primary_cluster_id}>
						<Select.Trigger class="w-full">
							{clusterLabel(primary_cluster_id) || 'Select cluster'}
						</Select.Trigger>
						<Select.Content>
							{#each clusters as c (c.id)}
								<Select.Item value={c.id} label={c.name}>{c.name}</Select.Item>
							{/each}
						</Select.Content>
					</Select.Root>
				</div>
				<div class="space-y-2">
					<Label for="replica-name">Name</Label>
					<Input id="replica-name" bind:value={name} required placeholder="dr-replica" />
				</div>
				<div class="space-y-2">
					<Label>Target node</Label>
					<Select.Root type="single" bind:value={target_node_id}>
						<Select.Trigger class="w-full">
							{nodeLabel(target_node_id) || 'Select node'}
						</Select.Trigger>
						<Select.Content>
							{#each nodes as n (n.id)}
								<Select.Item value={n.id} label={n.name}>{n.name}</Select.Item>
							{/each}
						</Select.Content>
					</Select.Root>
				</div>
				<div class="space-y-2">
					<Label>Mode</Label>
					<Select.Root type="single" bind:value={mode}>
						<Select.Trigger class="w-full font-mono text-sm">{mode}</Select.Trigger>
						<Select.Content>
							<Select.Item value="scheduled_sync" label="scheduled_sync">scheduled_sync</Select.Item>
							<Select.Item value="streaming" label="streaming">streaming</Select.Item>
							<Select.Item value="standby" label="standby">standby</Select.Item>
						</Select.Content>
					</Select.Root>
				</div>
				<div class="space-y-2">
					<Label>Sync cron</Label>
					<Input bind:value={sync_cron} class="font-mono" placeholder="*/30 * * * *" />
				</div>
				<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
					<span>Auto failover</span>
					<Switch bind:checked={auto_failover} />
				</label>
				<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
					<span>Provision now</span>
					<Switch bind:checked={provision_now} />
				</label>
				<Button type="submit" disabled={creating} class="w-full">
					<HugeiconsIcon icon={PlusSignIcon} class="size-4" strokeWidth={2} />
					{creating ? 'Creating…' : 'Create replica'}
				</Button>
			</form>
		</Card.Content>
	</Card.Root>

	<Card.Root class="border-border/60">
		<Card.Header>
			<Card.Title>
				{clusterFilter ? `Replicas · ${clusterName}` : 'All replicas'}
			</Card.Title>
			<Card.Description>{filteredReplicas.length} replica(s)</Card.Description>
		</Card.Header>
		<Card.Content class="p-0">
			{#if !loading && filteredReplicas.length === 0}
				<div class="p-6">
					<EmptyState title="No replicas" description="Create a replica from a primary cluster." />
				</div>
			{:else}
				<Table.Root>
					<Table.Header>
						<Table.Row class="hover:bg-transparent">
							<Table.Head>Name</Table.Head>
							<Table.Head>Primary</Table.Head>
							<Table.Head>Status</Table.Head>
							<Table.Head>Lag</Table.Head>
							<Table.Head class="text-right">Actions</Table.Head>
						</Table.Row>
					</Table.Header>
					<Table.Body>
						{#each filteredReplicas as r (r.id)}
							<Table.Row>
								<Table.Cell>
									<div class="font-medium">{r.name}</div>
									<div class="text-xs text-muted-foreground">
										{nodeLabel(r.target_node_id)} · {r.mode}
									</div>
									<div class="font-mono text-xs text-muted-foreground">{r.sync_cron}</div>
									{#if r.last_error}
										<div class="mt-1 text-xs text-destructive">{r.last_error}</div>
									{/if}
								</Table.Cell>
								<Table.Cell class="text-xs">
									<a href={`/clusters/${r.primary_cluster_id}`} class="hover:underline">
										{clusterLabel(r.primary_cluster_id)}
									</a>
								</Table.Cell>
								<Table.Cell>
									<StatusBadge status={r.status} />
								</Table.Cell>
								<Table.Cell class="tabular-nums text-xs">
									{#if r.lag_seconds != null}
										{r.lag_seconds}s
									{:else}
										—
									{/if}
									{#if r.last_sync_at}
										<div class="text-muted-foreground">{formatRelative(r.last_sync_at)}</div>
									{/if}
								</Table.Cell>
								<Table.Cell class="text-right">
									<div class="flex justify-end gap-1">
										<Button
											variant="ghost"
											size="sm"
											disabled={busy === r.id}
											onclick={() => syncReplica(r.id)}
										>
											<HugeiconsIcon icon={RefreshIcon} class="size-4" strokeWidth={2} />
											Sync
										</Button>
										<Button
											variant="ghost"
											size="sm"
											disabled={busy === r.id}
											onclick={() => askPromoteReplica(r.id)}
										>
											<HugeiconsIcon icon={ArrowUp01Icon} class="size-4" strokeWidth={2} />
											Promote
										</Button>
										<Button
											variant="ghost"
											size="sm"
											class="text-destructive hover:text-destructive"
											onclick={() => askDeleteReplica(r)}
										>
											<HugeiconsIcon icon={Delete02Icon} class="size-4" strokeWidth={2} />
										</Button>
									</div>
								</Table.Cell>
							</Table.Row>
						{/each}
					</Table.Body>
				</Table.Root>
			{/if}
		</Card.Content>
	</Card.Root>
</div>

<ConfirmDialog
	bind:open={promoteOpen}
	title="Promote replica to primary?"
	description="This is a destructive failover operation. The replica will become the new primary cluster."
	confirmLabel="Promote"
	variant="destructive"
	onConfirm={confirmPromoteReplica}
/>

<ConfirmDialog
	bind:open={deleteOpen}
	title="Delete replica?"
	description={deleteTarget ? `Delete replica "${deleteTarget.name}"?` : ''}
	confirmLabel="Delete"
	variant="destructive"
	onConfirm={confirmDeleteReplica}
/>
