<script lang="ts">
	import { onMount } from 'svelte';
	import { api, type Node, formatRelative } from '$lib/api';
	import PageHeader from '$lib/components/page-header.svelte';
	import StatusBadge from '$lib/components/status-badge.svelte';
	import EmptyState from '$lib/components/empty-state.svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Input } from '$lib/components/ui/input/index.js';
	import { Label } from '$lib/components/ui/label/index.js';
	import { Switch } from '$lib/components/ui/switch/index.js';
	import { Textarea } from '$lib/components/ui/textarea/index.js';
	import * as Card from '$lib/components/ui/card/index.js';
	import * as Table from '$lib/components/ui/table/index.js';
	import * as Alert from '$lib/components/ui/alert/index.js';
	import { Badge } from '$lib/components/ui/badge/index.js';
	import { toast } from 'svelte-sonner';
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import {
		Alert02Icon,
		CloudServerIcon,
		PlusSignIcon,
		RefreshIcon,
		Delete02Icon,
		StarIcon
	} from '@hugeicons/core-free-icons';

	let nodes = $state<Node[]>([]);
	let error = $state('');
	let loading = $state(true);
	let creating = $state(false);
	let pinging = $state<string | null>(null);

	let name = $state('');
	let docker_host = $state('tcp://');
	let notes = $state('');
	let max_clusters = $state<number | ''>('');
	let set_default = $state(false);

	async function load() {
		nodes = await api<Node[]>('/api/nodes');
	}

	onMount(() => {
		load()
			.catch((e) => (error = e instanceof Error ? e.message : 'Failed to load nodes'))
			.finally(() => (loading = false));
	});

	async function createNode(e: Event) {
		e.preventDefault();
		creating = true;
		error = '';
		try {
			await api<Node>('/api/nodes', {
				method: 'POST',
				body: JSON.stringify({
					name,
					docker_host,
					notes: notes || null,
					max_clusters: max_clusters === '' ? null : max_clusters,
					set_default
				})
			});
			toast.success('Node added');
			name = '';
			docker_host = 'tcp://';
			notes = '';
			max_clusters = '';
			set_default = false;
			await load();
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to create node';
			toast.error(error);
		} finally {
			creating = false;
		}
	}

	async function pingNode(id: string) {
		pinging = id;
		try {
			await api(`/api/nodes/${id}/ping`, { method: 'POST' });
			toast.success('Node is reachable');
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Ping failed');
			await load();
		} finally {
			pinging = null;
		}
	}

	async function setDefault(node: Node) {
		try {
			await api<Node>(`/api/nodes/${node.id}`, {
				method: 'PUT',
				body: JSON.stringify({ set_default: true })
			});
			toast.success(`"${node.name}" is now the default node`);
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to set default');
		}
	}

	async function deleteNode(node: Node) {
		if (!confirm(`Delete remote node "${node.name}"? This cannot be undone.`)) return;
		try {
			await api(`/api/nodes/${node.id}`, { method: 'DELETE' });
			toast.success('Node deleted');
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Delete failed');
		}
	}
</script>

<PageHeader
	title="Nodes"
	description="Multi-host Docker endpoints for provisioning clusters across machines"
/>

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
				<HugeiconsIcon icon={CloudServerIcon} class="size-5 text-primary" strokeWidth={2} />
				Add remote node
			</Card.Title>
			<Card.Description>
				Connect a remote Docker host via TCP. The panel probes connectivity before saving.
			</Card.Description>
		</Card.Header>
		<Card.Content>
			<form class="space-y-4" onsubmit={createNode}>
				<div class="space-y-2">
					<Label for="node-name">Display name</Label>
					<Input id="node-name" bind:value={name} placeholder="Worker 1" required />
				</div>
				<div class="space-y-2">
					<Label for="docker-host">Docker host</Label>
					<Input
						id="docker-host"
						bind:value={docker_host}
						class="font-mono text-sm"
						placeholder="tcp://192.168.1.10:2375"
						required
					/>
				</div>
				<div class="space-y-2">
					<Label for="notes">Notes</Label>
					<Textarea id="notes" bind:value={notes} rows={2} placeholder="Optional operator notes" />
				</div>
				<div class="space-y-2">
					<Label for="max-clusters">Max clusters <span class="text-muted-foreground">(optional)</span></Label>
					<Input
						id="max-clusters"
						type="number"
						min="1"
						bind:value={max_clusters}
						placeholder="Unlimited"
					/>
				</div>
				<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
					<span>Set as default node for new clusters</span>
					<Switch bind:checked={set_default} />
				</label>
				<Button type="submit" disabled={creating} class="w-full">
					<HugeiconsIcon icon={PlusSignIcon} class="size-4" strokeWidth={2} />
					{creating ? 'Adding…' : 'Add node'}
				</Button>
			</form>
		</Card.Content>
	</Card.Root>

	<Card.Root class="border-border/60">
		<Card.Header class="flex-row items-center justify-between space-y-0">
			<div>
				<Card.Title>Registered nodes</Card.Title>
				<Card.Description>Built-in local node plus any remote Docker hosts</Card.Description>
			</div>
			<Button variant="outline" size="sm" onclick={() => load()}>
				<HugeiconsIcon icon={RefreshIcon} class="size-4" strokeWidth={2} />
				Refresh
			</Button>
		</Card.Header>
		<Card.Content class="p-0">
			{#if !loading && nodes.length === 0}
				<div class="p-6">
					<EmptyState title="No nodes" description="Add a remote Docker host to scale beyond this machine." />
				</div>
			{:else}
				<Table.Root>
					<Table.Header>
						<Table.Row class="hover:bg-transparent">
							<Table.Head>Name</Table.Head>
							<Table.Head>Host</Table.Head>
							<Table.Head>Status</Table.Head>
							<Table.Head>Clusters</Table.Head>
							<Table.Head class="text-right">Actions</Table.Head>
						</Table.Row>
					</Table.Header>
					<Table.Body>
						{#each nodes as node (node.id)}
							<Table.Row>
								<Table.Cell>
									<div class="flex items-center gap-2">
										<span class="font-medium">{node.name}</span>
										{#if node.is_default}
											<Badge variant="secondary" class="text-[10px]">default</Badge>
										{/if}
										{#if node.kind === 'local'}
											<Badge variant="outline" class="text-[10px]">local</Badge>
										{/if}
									</div>
									<div class="font-mono text-xs text-muted-foreground">{node.slug}</div>
									{#if node.last_seen_at}
										<div class="text-xs text-muted-foreground">
											Seen {formatRelative(node.last_seen_at)}
										</div>
									{/if}
									{#if node.last_error}
										<div class="mt-1 text-xs text-destructive">{node.last_error}</div>
									{/if}
								</Table.Cell>
								<Table.Cell class="font-mono text-xs">
									{node.docker_host_display ?? '—'}
								</Table.Cell>
								<Table.Cell>
									<StatusBadge status={node.status} />
								</Table.Cell>
								<Table.Cell class="tabular-nums">
									{node.cluster_count}{#if node.max_clusters != null}
										<span class="text-muted-foreground"> / {node.max_clusters}</span>
									{/if}
								</Table.Cell>
								<Table.Cell class="text-right">
									<div class="flex justify-end gap-1">
										<Button
											variant="ghost"
											size="sm"
											disabled={pinging === node.id}
											onclick={() => pingNode(node.id)}
										>
											<HugeiconsIcon icon={RefreshIcon} class="size-4" strokeWidth={2} />
											{pinging === node.id ? '…' : 'Ping'}
										</Button>
										{#if !node.is_default}
											<Button variant="ghost" size="sm" onclick={() => setDefault(node)}>
												<HugeiconsIcon icon={StarIcon} class="size-4" strokeWidth={2} />
												Default
											</Button>
										{/if}
										{#if node.kind === 'remote'}
											<Button
												variant="ghost"
												size="sm"
												class="text-destructive hover:text-destructive"
												onclick={() => deleteNode(node)}
											>
												<HugeiconsIcon icon={Delete02Icon} class="size-4" strokeWidth={2} />
												Delete
											</Button>
										{/if}
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
