<script lang="ts">
	import { onMount } from 'svelte';
	import {
		api,
		type Node,
		type FleetRemoteAccess,
		type FleetPeer,
		formatRelative,
		formatNodeCapacity,
		isUnlimitedCapacity
	} from '$lib/api';
	import PageHeader from '$lib/components/page-header.svelte';
	import StatusBadge from '$lib/components/status-badge.svelte';
	import EmptyState from '$lib/components/empty-state.svelte';
	import ConfirmDialog from '$lib/components/confirm-dialog.svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Input } from '$lib/components/ui/input/index.js';
	import { Label } from '$lib/components/ui/label/index.js';
	import { Switch } from '$lib/components/ui/switch/index.js';
	import { Textarea } from '$lib/components/ui/textarea/index.js';
	import * as Card from '$lib/components/ui/card/index.js';
	import * as Table from '$lib/components/ui/table/index.js';
	import * as Alert from '$lib/components/ui/alert/index.js';
	import * as Dialog from '$lib/components/ui/dialog/index.js';
	import { Badge } from '$lib/components/ui/badge/index.js';
	import { toast } from 'svelte-sonner';
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import {
		Alert02Icon,
		CloudServerIcon,
		PlusSignIcon,
		RefreshIcon,
		Delete02Icon,
		StarIcon,
		Edit02Icon,
		Copy01Icon,
		Link01Icon,
		Globe02Icon
	} from '@hugeicons/core-free-icons';

	let nodes = $state<Node[]>([]);
	let error = $state('');
	let loading = $state(true);
	let pinging = $state<string | null>(null);

	// Create dialog
	let createOpen = $state(false);
	let creating = $state(false);
	let createName = $state('');
	let createDockerHost = $state('tcp://');
	let createNotes = $state('');
	let createMaxClusters = $state('');
	let createSetDefault = $state(false);

	// Edit dialog
	let editOpen = $state(false);
	let editing = $state(false);
	let editNode = $state<Node | null>(null);
	let editName = $state('');
	let editNotes = $state('');
	let editMaxClusters = $state('');
	let editSetDefault = $state(false);
	let editDockerHost = $state('');

	// Delete confirm
	let deleteOpen = $state(false);
	let deleteTarget = $state<Node | null>(null);

	// Fleet
	let fleetAccess = $state<FleetRemoteAccess | null>(null);
	let fleetPeers = $state<FleetPeer[]>([]);
	let fleetLoading = $state(false);
	let fleetSaving = $state(false);
	let fleetPublicUrl = $state('');
	let fleetPublicName = $state('');
	let fleetEnabled = $state(false);
	let joinLink = $state('');
	let joining = $state(false);
	let peerPinging = $state<string | null>(null);
	let deletePeerOpen = $state(false);
	let deletePeerTarget = $state<FleetPeer | null>(null);
	let rotateConfirmOpen = $state(false);

	function defaultPublicUrl(): string {
		if (typeof window !== 'undefined') return window.location.origin;
		return '';
	}

	async function parsePeers(raw: unknown): Promise<FleetPeer[]> {
		if (Array.isArray(raw)) return raw as FleetPeer[];
		if (raw && typeof raw === 'object' && 'peers' in raw) {
			const peers = (raw as { peers?: FleetPeer[] }).peers;
			return peers ?? [];
		}
		return [];
	}

	async function loadNodes() {
		nodes = await api<Node[]>('/api/nodes');
	}

	async function loadFleet() {
		fleetLoading = true;
		try {
			const [access, peersRaw] = await Promise.all([
				api<FleetRemoteAccess>('/api/fleet/remote-access'),
				api<unknown>('/api/fleet/peers').catch(() => [])
			]);
			fleetAccess = access;
			fleetEnabled = access.enabled;
			fleetPublicUrl = access.public_base_url?.trim() || defaultPublicUrl();
			fleetPublicName = access.public_name ?? '';
			fleetPeers = await parsePeers(peersRaw);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load fleet settings');
		} finally {
			fleetLoading = false;
		}
	}

	async function load() {
		await loadNodes();
		await loadFleet();
	}

	onMount(() => {
		load()
			.catch((e) => (error = e instanceof Error ? e.message : 'Failed to load'))
			.finally(() => (loading = false));
	});

	function maxClustersPayload(value: string): number | null {
		if (value.trim() === '') return null;
		const n = Number(value);
		return Number.isFinite(n) && n > 0 ? n : null;
	}

	function maxClustersUpdatePayload(value: string): number {
		if (value.trim() === '') return 0;
		const n = Number(value);
		return Number.isFinite(n) && n > 0 ? n : 0;
	}

	function openCreate() {
		createName = '';
		createDockerHost = 'tcp://';
		createNotes = '';
		createMaxClusters = '';
		createSetDefault = false;
		createOpen = true;
	}

	async function createNode(e: Event) {
		e.preventDefault();
		creating = true;
		error = '';
		try {
			await api<Node>('/api/nodes', {
				method: 'POST',
				body: JSON.stringify({
					name: createName,
					docker_host: createDockerHost,
					notes: createNotes || null,
					max_clusters: maxClustersPayload(createMaxClusters),
					set_default: createSetDefault
				})
			});
			toast.success('Docker host added');
			createOpen = false;
			await loadNodes();
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to create host';
			toast.error(error);
		} finally {
			creating = false;
		}
	}

	function openEdit(node: Node) {
		editNode = node;
		editName = node.name;
		editNotes = node.notes ?? '';
		editMaxClusters = isUnlimitedCapacity(node.max_clusters) ? '' : String(node.max_clusters);
		editSetDefault = node.is_default;
		editDockerHost = node.docker_host_display ?? '';
		editOpen = true;
	}

	async function saveEdit(e: Event) {
		e.preventDefault();
		if (!editNode) return;
		editing = true;
		try {
			const body: Record<string, unknown> = {
				name: editName,
				notes: editNotes || null,
				max_clusters: maxClustersUpdatePayload(editMaxClusters),
				set_default: editSetDefault
			};
			if (editNode.kind === 'remote' && editDockerHost.trim()) {
				body.docker_host = editDockerHost;
			}
			await api<Node>(`/api/nodes/${editNode.id}`, {
				method: 'PUT',
				body: JSON.stringify(body)
			});
			toast.success('Host updated');
			editOpen = false;
			await loadNodes();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Update failed');
		} finally {
			editing = false;
		}
	}

	function askDelete(node: Node) {
		deleteTarget = node;
		deleteOpen = true;
	}

	async function confirmDelete() {
		if (!deleteTarget) return;
		await api(`/api/nodes/${deleteTarget.id}`, { method: 'DELETE' });
		toast.success('Host deleted');
		deleteTarget = null;
		await loadNodes();
	}

	async function pingNode(id: string) {
		pinging = id;
		try {
			await api(`/api/nodes/${id}/ping`, { method: 'POST' });
			toast.success('Host is reachable');
			await loadNodes();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Ping failed');
			await loadNodes();
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
			toast.success(`"${node.name}" is now the default host`);
			await loadNodes();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to set default');
		}
	}

	async function saveFleetAccess() {
		fleetSaving = true;
		try {
			const body: Record<string, unknown> = {
				enabled: fleetEnabled,
				public_base_url: fleetPublicUrl.trim() || defaultPublicUrl()
			};
			if (fleetPublicName.trim()) {
				body.public_name = fleetPublicName.trim();
			}
			fleetAccess = await api<FleetRemoteAccess>('/api/fleet/remote-access', {
				method: 'PUT',
				body: JSON.stringify(body)
			});
			fleetEnabled = fleetAccess.enabled;
			fleetPublicUrl = fleetAccess.public_base_url?.trim() || defaultPublicUrl();
			fleetPublicName = fleetAccess.public_name ?? fleetPublicName;
			toast.success(
				fleetAccess.enabled
					? fleetAccess.join_url
						? 'Node mode on — copy the join link below'
						: 'Node mode enabled'
					: 'Node mode disabled'
			);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to save');
		} finally {
			fleetSaving = false;
		}
	}

	async function rotateToken() {
		try {
			fleetAccess = await api<FleetRemoteAccess>('/api/fleet/remote-access/rotate-token', {
				method: 'POST'
			});
			fleetEnabled = fleetAccess.enabled;
			toast.success('New join link created');
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Rotate failed');
		}
	}

	async function joinPanel(e: Event) {
		e.preventDefault();
		const link = joinLink.trim();
		if (!link) {
			toast.error('Paste a join link first');
			return;
		}
		joining = true;
		try {
			await api('/api/fleet/join', {
				method: 'POST',
				body: JSON.stringify({ link })
			});
			toast.success('Connected to remote panel');
			joinLink = '';
			await loadFleet();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Connect failed');
		} finally {
			joining = false;
		}
	}

	async function pingPeer(id: string) {
		peerPinging = id;
		try {
			const res = await api<{ latency_ms?: number }>(`/api/fleet/peers/${id}/ping`, {
				method: 'POST'
			});
			toast.success(
				res.latency_ms != null ? `Peer reachable (${res.latency_ms} ms)` : 'Peer reachable'
			);
			await loadFleet();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Ping failed');
		} finally {
			peerPinging = null;
		}
	}

	async function removePeer() {
		if (!deletePeerTarget) return;
		await api(`/api/fleet/peers/${deletePeerTarget.id}`, { method: 'DELETE' });
		fleetPeers = fleetPeers.filter((p) => p.id !== deletePeerTarget!.id);
		toast.success('Peer removed');
		deletePeerTarget = null;
	}

	async function copyText(text: string, label: string) {
		try {
			await navigator.clipboard.writeText(text);
			toast.success(`${label} copied`);
		} catch {
			toast.error('Could not copy — select manually');
		}
	}
</script>

<PageHeader
	title="Fleet"
	description="Pair PgPanel instances with a single join link, or manage local Docker hosts"
>
	{#snippet actions()}
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

<div class="space-y-6">
	<!-- Block 1: Node mode -->
	<Card.Root class="page-card border-primary/20">
		<Card.Header>
			<Card.Title class="flex items-center gap-2">
				<HugeiconsIcon icon={Globe02Icon} class="size-5 text-primary" strokeWidth={2} />
				Node mode (this panel)
			</Card.Title>
			<Card.Description>
				Let another PgPanel connect to this instance. Share one join link — no separate invites.
			</Card.Description>
		</Card.Header>
		<Card.Content class="space-y-4">
			{#if fleetLoading && !fleetAccess}
				<p class="text-sm text-muted-foreground">Loading…</p>
			{:else}
				<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
					<span>Enable node mode</span>
					<Switch bind:checked={fleetEnabled} />
				</label>

				<div class="grid gap-4 sm:grid-cols-2">
					<div class="space-y-2">
						<Label for="fleet-url">Public URL</Label>
						<Input
							id="fleet-url"
							bind:value={fleetPublicUrl}
							placeholder="https://panel.example.com"
							class="font-mono text-sm"
						/>
					</div>
					<div class="space-y-2">
						<Label for="fleet-name">
							Display name <span class="text-muted-foreground">(optional)</span>
						</Label>
						<Input
							id="fleet-name"
							bind:value={fleetPublicName}
							placeholder="Production panel"
						/>
					</div>
				</div>

				<Button onclick={saveFleetAccess} disabled={fleetSaving}>
					{fleetSaving ? 'Saving…' : fleetEnabled ? 'Save & enable' : 'Save'}
				</Button>

				{#if fleetAccess?.enabled && fleetAccess.join_url}
					<div class="space-y-3 rounded-lg border border-primary/30 bg-primary/5 p-4">
						<p class="text-sm font-medium">Join link</p>
						<div class="flex flex-col gap-2 sm:flex-row sm:items-center">
							<Input
								readonly
								value={fleetAccess.join_url}
								class="font-mono text-sm sm:text-base"
							/>
							<div class="flex shrink-0 gap-2">
								<Button
									variant="default"
									onclick={() => copyText(fleetAccess!.join_url!, 'Join link')}
								>
									<HugeiconsIcon icon={Copy01Icon} class="size-4" strokeWidth={2} />
									Copy
								</Button>
								<Button variant="outline" onclick={() => (rotateConfirmOpen = true)}>
									New link
								</Button>
							</div>
						</div>
						<p class="text-xs text-muted-foreground">
							Share this link with another PgPanel. Paste it there to connect.
						</p>
					</div>
				{/if}
			{/if}
		</Card.Content>
	</Card.Root>

	<!-- Block 2: Connect another panel -->
	<Card.Root class="page-card">
		<Card.Header>
			<Card.Title class="flex items-center gap-2">
				<HugeiconsIcon icon={Link01Icon} class="size-5" strokeWidth={2} />
				Connect another panel
			</Card.Title>
			<Card.Description>Paste the join link from another PgPanel</Card.Description>
		</Card.Header>
		<Card.Content>
			<form class="space-y-4" onsubmit={joinPanel}>
				<div class="space-y-2">
					<Label for="join-link">Join link</Label>
					<Textarea
						id="join-link"
						bind:value={joinLink}
						rows={3}
						placeholder="https://other-panel.example.com/join?token=fleet_…"
						class="font-mono text-sm"
					/>
				</div>
				<Button type="submit" disabled={joining || !joinLink.trim()}>
					{joining ? 'Connecting…' : 'Connect'}
				</Button>
			</form>
		</Card.Content>
	</Card.Root>

	<!-- Block 3: Connected panels -->
	<Card.Root class="page-card">
		<Card.Header>
			<Card.Title>Connected panels</Card.Title>
			<Card.Description>Remote PgPanel instances paired with this one</Card.Description>
		</Card.Header>
		<Card.Content class="p-0">
			{#if fleetPeers.length === 0}
				<p class="p-4 text-sm text-muted-foreground">No connected panels yet.</p>
			{:else}
				<div class="table-scroll">
					<Table.Root>
						<Table.Header>
							<Table.Row class="hover:bg-transparent">
								<Table.Head>Name</Table.Head>
								<Table.Head>Status</Table.Head>
								<Table.Head>URL</Table.Head>
								<Table.Head>Clusters</Table.Head>
								<Table.Head>Last seen</Table.Head>
								<Table.Head class="text-right">Actions</Table.Head>
							</Table.Row>
						</Table.Header>
						<Table.Body>
							{#each fleetPeers as peer (peer.id)}
								<Table.Row>
									<Table.Cell class="font-medium">{peer.name}</Table.Cell>
									<Table.Cell>
										<StatusBadge status={peer.status} />
										{#if peer.last_ping_ms != null}
											<span class="ml-1 text-xs text-muted-foreground">
												{peer.last_ping_ms} ms
											</span>
										{/if}
									</Table.Cell>
									<Table.Cell class="max-w-[14rem] truncate font-mono text-xs">
										{peer.base_url}
									</Table.Cell>
									<Table.Cell class="tabular-nums text-xs">
										{#if peer.cluster_count != null}
											{formatNodeCapacity(peer.cluster_count, peer.max_clusters)}
										{:else}
											—
										{/if}
									</Table.Cell>
									<Table.Cell class="text-xs text-muted-foreground">
										{peer.last_seen_at ? formatRelative(peer.last_seen_at) : '—'}
									</Table.Cell>
									<Table.Cell class="text-right">
										<div class="flex justify-end gap-1">
											<Button
												variant="ghost"
												size="sm"
												disabled={peerPinging === peer.id}
												onclick={() => pingPeer(peer.id)}
											>
												{peerPinging === peer.id ? '…' : 'Ping'}
											</Button>
											<Button
												variant="ghost"
												size="sm"
												class="text-destructive hover:text-destructive"
												onclick={() => {
													deletePeerTarget = peer;
													deletePeerOpen = true;
												}}
											>
												Remove
											</Button>
										</div>
									</Table.Cell>
								</Table.Row>
							{/each}
						</Table.Body>
					</Table.Root>
				</div>
			{/if}
		</Card.Content>
	</Card.Root>

	<!-- Advanced: Docker hosts -->
	<details class="group rounded-xl border border-border/60 bg-card/40">
		<summary class="cursor-pointer select-none px-4 py-3 text-sm font-medium text-muted-foreground hover:text-foreground">
			Docker hosts (advanced) — multi-host Docker on this panel only
		</summary>
		<div class="space-y-4 border-t border-border/60 p-4">
			<div class="flex items-center justify-between gap-4">
				<p class="text-sm text-muted-foreground">
					Add remote Docker hosts when this panel manages more than one machine.
				</p>
				<Button size="sm" onclick={openCreate}>
					<HugeiconsIcon icon={PlusSignIcon} class="size-4" strokeWidth={2} />
					Add host
				</Button>
			</div>

			{#if !loading && nodes.length === 0}
				<EmptyState
					title="No Docker hosts"
					description="The local host is always available. Add remote hosts only if needed."
				/>
			{:else}
				<div class="table-scroll rounded-lg border border-border/60">
					<Table.Root>
						<Table.Header>
							<Table.Row class="hover:bg-transparent">
								<Table.Head>Name</Table.Head>
								<Table.Head>Host</Table.Head>
								<Table.Head>Status</Table.Head>
								<Table.Head>Capacity</Table.Head>
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
									<Table.Cell class="tabular-nums whitespace-nowrap text-xs">
										{formatNodeCapacity(node.cluster_count, node.max_clusters)}
									</Table.Cell>
									<Table.Cell class="text-right">
										<div class="flex justify-end gap-1">
											<Button variant="ghost" size="sm" onclick={() => openEdit(node)}>
												<HugeiconsIcon icon={Edit02Icon} class="size-4" strokeWidth={2} />
											</Button>
											<Button
												variant="ghost"
												size="sm"
												disabled={pinging === node.id}
												onclick={() => pingNode(node.id)}
											>
												{pinging === node.id ? '…' : 'Ping'}
											</Button>
											{#if !node.is_default}
												<Button variant="ghost" size="sm" onclick={() => setDefault(node)}>
													<HugeiconsIcon icon={StarIcon} class="size-4" strokeWidth={2} />
												</Button>
											{/if}
											{#if node.kind === 'remote'}
												<Button
													variant="ghost"
													size="sm"
													class="text-destructive hover:text-destructive"
													onclick={() => askDelete(node)}
												>
													<HugeiconsIcon icon={Delete02Icon} class="size-4" strokeWidth={2} />
												</Button>
											{/if}
										</div>
									</Table.Cell>
								</Table.Row>
							{/each}
						</Table.Body>
					</Table.Root>
				</div>
			{/if}
		</div>
	</details>
</div>

<Dialog.Root bind:open={createOpen}>
	<Dialog.Content class="sm:max-w-lg">
		<Dialog.Header>
			<Dialog.Title class="flex items-center gap-2">
				<HugeiconsIcon icon={CloudServerIcon} class="size-5 text-primary" strokeWidth={2} />
				Add Docker host
			</Dialog.Title>
			<Dialog.Description>Remote Docker host via TCP</Dialog.Description>
		</Dialog.Header>
		<form class="space-y-4" onsubmit={createNode}>
			<div class="space-y-2">
				<Label for="create-name">Display name</Label>
				<Input id="create-name" bind:value={createName} placeholder="Worker 1" required />
			</div>
			<div class="space-y-2">
				<Label for="create-docker">Docker host</Label>
				<Input
					id="create-docker"
					bind:value={createDockerHost}
					class="font-mono text-sm"
					placeholder="tcp://192.168.1.10:2375"
					required
				/>
			</div>
			<div class="space-y-2">
				<Label for="create-notes">Notes</Label>
				<Textarea id="create-notes" bind:value={createNotes} rows={2} placeholder="Optional" />
			</div>
			<div class="space-y-2">
				<Label for="create-max">
					Max clusters <span class="text-muted-foreground">(empty = unlimited)</span>
				</Label>
				<Input
					id="create-max"
					type="number"
					min="1"
					bind:value={createMaxClusters}
					placeholder="Unlimited"
				/>
			</div>
			<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
				<span>Default host for new clusters</span>
				<Switch bind:checked={createSetDefault} />
			</label>
			<Dialog.Footer class="gap-2 border-t-0 bg-transparent sm:justify-end">
				<Button type="button" variant="outline" onclick={() => (createOpen = false)}>Cancel</Button>
				<Button type="submit" disabled={creating}>
					{creating ? 'Adding…' : 'Add host'}
				</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>

<Dialog.Root bind:open={editOpen}>
	<Dialog.Content class="sm:max-w-lg">
		<Dialog.Header>
			<Dialog.Title>Edit Docker host</Dialog.Title>
			<Dialog.Description>
				{#if editNode?.kind === 'local'}
					Local host — Docker endpoint cannot be changed here.
				{:else}
					Update remote host settings.
				{/if}
			</Dialog.Description>
		</Dialog.Header>
		{#if editNode}
			<form class="space-y-4" onsubmit={saveEdit}>
				<div class="space-y-2">
					<Label for="edit-name">Display name</Label>
					<Input id="edit-name" bind:value={editName} required />
				</div>
				{#if editNode.kind === 'remote'}
					<div class="space-y-2">
						<Label for="edit-docker">Docker host</Label>
						<Input id="edit-docker" bind:value={editDockerHost} class="font-mono text-sm" required />
					</div>
				{/if}
				<div class="space-y-2">
					<Label for="edit-notes">Notes</Label>
					<Textarea id="edit-notes" bind:value={editNotes} rows={2} />
				</div>
				<div class="space-y-2">
					<Label for="edit-max">
						Max clusters <span class="text-muted-foreground">(empty = unlimited)</span>
					</Label>
					<Input
						id="edit-max"
						type="number"
						min="1"
						bind:value={editMaxClusters}
						placeholder="Unlimited"
					/>
				</div>
				<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
					<span>Default host for new clusters</span>
					<Switch bind:checked={editSetDefault} />
				</label>
				<Dialog.Footer class="gap-2 border-t-0 bg-transparent sm:justify-end">
					<Button type="button" variant="outline" onclick={() => (editOpen = false)}>Cancel</Button>
					<Button type="submit" disabled={editing}>{editing ? 'Saving…' : 'Save'}</Button>
				</Dialog.Footer>
			</form>
		{/if}
	</Dialog.Content>
</Dialog.Root>

<ConfirmDialog
	bind:open={deleteOpen}
	title="Delete Docker host?"
	description={deleteTarget ? `Delete "${deleteTarget.name}"? This cannot be undone.` : ''}
	confirmLabel="Delete"
	variant="destructive"
	onConfirm={confirmDelete}
/>

<ConfirmDialog
	bind:open={rotateConfirmOpen}
	title="Create new join link?"
	description="The current link will stop working. Connected panels stay paired."
	confirmLabel="New link"
	variant="destructive"
	onConfirm={rotateToken}
/>

<ConfirmDialog
	bind:open={deletePeerOpen}
	title="Remove panel?"
	description={deletePeerTarget ? `Disconnect from "${deletePeerTarget.name}"?` : ''}
	confirmLabel="Remove"
	variant="destructive"
	onConfirm={removePeer}
/>
