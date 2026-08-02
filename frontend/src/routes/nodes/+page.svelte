<script lang="ts">
	import { onMount } from 'svelte';
	import {
		api,
		type Node,
		type FleetRemoteAccess,
		type FleetInvite,
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
	import * as Tabs from '$lib/components/ui/tabs/index.js';
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
		Globe02Icon,
		InformationCircleIcon
	} from '@hugeicons/core-free-icons';

	let activeTab = $state('nodes');
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
	let fleetInvites = $state<FleetInvite[]>([]);
	let fleetPeers = $state<FleetPeer[]>([]);
	let fleetLoading = $state(false);
	let fleetSaving = $state(false);
	let fleetPublicUrl = $state('');
	let fleetEnabled = $state(false);
	let joinBaseUrl = $state('');
	let joinToken = $state('');
	let joinLocalName = $state('');
	let joining = $state(false);
	let peerPinging = $state<string | null>(null);
	let deletePeerOpen = $state(false);
	let deletePeerTarget = $state<FleetPeer | null>(null);
	let deleteInviteOpen = $state(false);
	let deleteInviteTarget = $state<FleetInvite | null>(null);
	let rotateConfirmOpen = $state(false);

	async function load() {
		nodes = await api<Node[]>('/api/nodes');
	}

	async function loadFleet() {
		fleetLoading = true;
		try {
			const [access, invites, peers] = await Promise.all([
				api<FleetRemoteAccess>('/api/fleet/remote-access'),
				api<FleetInvite[]>('/api/fleet/invites').catch(() => [] as FleetInvite[]),
				api<FleetPeer[]>('/api/fleet/peers').catch(() => [] as FleetPeer[])
			]);
			fleetAccess = access;
			fleetEnabled = access.enabled;
			fleetPublicUrl = access.public_base_url ?? '';
			fleetInvites = invites;
			fleetPeers = peers;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load fleet settings');
		} finally {
			fleetLoading = false;
		}
	}

	onMount(() => {
		load()
			.catch((e) => (error = e instanceof Error ? e.message : 'Failed to load nodes'))
			.finally(() => (loading = false));
		loadFleet();
	});

	function maxClustersPayload(value: string): number | null {
		if (value.trim() === '') return null;
		const n = Number(value);
		return Number.isFinite(n) && n > 0 ? n : null;
	}

	/** For updates: empty means unlimited → send 0 (backend clears to NULL). */
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
			toast.success('Node added');
			createOpen = false;
			await load();
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to create node';
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
			toast.success('Node updated');
			editOpen = false;
			await load();
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
		toast.success('Node deleted');
		deleteTarget = null;
		await load();
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

	async function saveFleetAccess() {
		fleetSaving = true;
		try {
			fleetAccess = await api<FleetRemoteAccess>('/api/fleet/remote-access', {
				method: 'PUT',
				body: JSON.stringify({
					enabled: fleetEnabled,
					public_base_url: fleetPublicUrl.trim() || null
				})
			});
			fleetEnabled = fleetAccess.enabled;
			fleetPublicUrl = fleetAccess.public_base_url ?? '';
			toast.success('Remote access settings saved');
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
			toast.success('Join token rotated');
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Rotate failed');
		}
	}

	async function createInvite() {
		try {
			const invite = await api<FleetInvite>('/api/fleet/invites', {
				method: 'POST',
				body: JSON.stringify({})
			});
			fleetInvites = [invite, ...fleetInvites];
			toast.success('Invite created');
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create invite');
		}
	}

	async function deleteInvite() {
		if (!deleteInviteTarget) return;
		await api(`/api/fleet/invites/${deleteInviteTarget.id}`, { method: 'DELETE' });
		fleetInvites = fleetInvites.filter((i) => i.id !== deleteInviteTarget!.id);
		toast.success('Invite removed');
		deleteInviteTarget = null;
	}

	async function joinPanel(e: Event) {
		e.preventDefault();
		joining = true;
		try {
			await api('/api/fleet/join', {
				method: 'POST',
				body: JSON.stringify({
					base_url: joinBaseUrl.trim(),
					token: joinToken.trim(),
					local_name: joinLocalName.trim() || undefined
				})
			});
			toast.success('Joined remote panel');
			joinBaseUrl = '';
			joinToken = '';
			joinLocalName = '';
			await loadFleet();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Join failed');
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
			toast.success(res.latency_ms != null ? `Peer reachable (${res.latency_ms} ms)` : 'Peer reachable');
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
	title="Nodes & Fleet"
	description="Docker hosts, capacity limits, and multi-panel remote access"
>
	{#snippet actions()}
		<Button variant="outline" size="sm" onclick={() => { load(); loadFleet(); }}>
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

<Tabs.Root bind:value={activeTab} class="space-y-6">
	<Tabs.List>
		<Tabs.Trigger value="nodes">Nodes</Tabs.Trigger>
		<Tabs.Trigger value="fleet">Fleet / Remote access</Tabs.Trigger>
	</Tabs.List>

	<Tabs.Content value="nodes" class="space-y-4">
		<Alert.Root class="border-border/60">
			<HugeiconsIcon icon={InformationCircleIcon} class="size-4" strokeWidth={2} />
			<Alert.Description>
				Each node enforces its own <strong>max clusters</strong> limit locally. Fleet peers may
				report capacity, but provisioning is always checked on the target host.
			</Alert.Description>
		</Alert.Root>

		<Card.Root class="page-card">
			<Card.Header class="flex-row items-center justify-between gap-4 space-y-0">
				<div>
					<Card.Title>Registered nodes</Card.Title>
					<Card.Description>Local node and remote Docker hosts</Card.Description>
				</div>
				<Button size="sm" onclick={openCreate}>
					<HugeiconsIcon icon={PlusSignIcon} class="size-4" strokeWidth={2} />
					Add remote node
				</Button>
			</Card.Header>
			<Card.Content class="p-0">
				{#if !loading && nodes.length === 0}
					<div class="p-6">
						<EmptyState
							title="No nodes"
							description="Add a remote Docker host to scale beyond this machine."
						/>
					</div>
				{:else}
					<div class="table-scroll">
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
											<div class="font-mono text-xs text-muted-foreground">{node.slug}</div>
											{#if node.notes}
												<div class="mt-0.5 text-xs text-muted-foreground line-clamp-2">{node.notes}</div>
											{/if}
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
										<Table.Cell class="tabular-nums whitespace-nowrap">
											{formatNodeCapacity(node.cluster_count, node.max_clusters)}
										</Table.Cell>
										<Table.Cell class="text-right">
											<div class="flex justify-end gap-1">
												<Button variant="ghost" size="sm" onclick={() => openEdit(node)}>
													<HugeiconsIcon icon={Edit02Icon} class="size-4" strokeWidth={2} />
													Edit
												</Button>
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
														onclick={() => askDelete(node)}
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
					</div>
				{/if}
			</Card.Content>
		</Card.Root>
	</Tabs.Content>

	<Tabs.Content value="fleet" class="space-y-6">
		<Alert.Root class="border-border/60">
			<HugeiconsIcon icon={InformationCircleIcon} class="size-4" strokeWidth={2} />
			<Alert.Description>
				Connect multiple PgPanel instances into a fleet. Capacity limits are configured per node on
				each panel — peers share visibility but enforce limits on their own hosts.
			</Alert.Description>
		</Alert.Root>

		<Card.Root class="page-card">
			<Card.Header>
				<Card.Title class="flex items-center gap-2">
					<HugeiconsIcon icon={Globe02Icon} class="size-5 text-primary" strokeWidth={2} />
					Remote access
				</Card.Title>
				<Card.Description>
					Allow other panels to join this instance via a public URL and token
				</Card.Description>
			</Card.Header>
			<Card.Content class="space-y-4">
				{#if fleetLoading && !fleetAccess}
					<p class="text-sm text-muted-foreground">Loading fleet settings…</p>
				{:else}
					<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
						<span>Enable remote access</span>
						<Switch bind:checked={fleetEnabled} />
					</label>
					<div class="space-y-2">
						<Label for="fleet-url">Public base URL</Label>
						<Input
							id="fleet-url"
							bind:value={fleetPublicUrl}
							placeholder="https://panel.example.com"
							class="font-mono text-sm"
						/>
					</div>
					<Button onclick={saveFleetAccess} disabled={fleetSaving}>
						{fleetSaving ? 'Saving…' : 'Save remote access'}
					</Button>

					{#if fleetAccess?.enabled && (fleetAccess.join_url || fleetAccess.join_token)}
						<div class="space-y-3 rounded-lg border border-border/60 bg-muted/20 p-4">
							<p class="text-sm font-medium">Join credentials</p>
							{#if fleetAccess.join_url}
								<div class="flex items-center gap-2">
									<Input readonly value={fleetAccess.join_url} class="font-mono text-xs" />
									<Button
										variant="outline"
										size="icon"
										onclick={() => copyText(fleetAccess!.join_url!, 'Join link')}
									>
										<HugeiconsIcon icon={Copy01Icon} class="size-4" strokeWidth={2} />
									</Button>
								</div>
							{/if}
							{#if fleetAccess.join_token}
								<div class="flex items-center gap-2">
									<Input readonly value={fleetAccess.join_token} class="font-mono text-xs" />
									<Button
										variant="outline"
										size="icon"
										onclick={() => copyText(fleetAccess!.join_token!, 'Token')}
									>
										<HugeiconsIcon icon={Copy01Icon} class="size-4" strokeWidth={2} />
									</Button>
								</div>
							{/if}
							<Button variant="outline" size="sm" onclick={() => (rotateConfirmOpen = true)}>
								<HugeiconsIcon icon={RefreshIcon} class="size-4" strokeWidth={2} />
								Rotate token
							</Button>
						</div>
					{/if}
				{/if}
			</Card.Content>
		</Card.Root>

		<div class="grid gap-6 lg:grid-cols-2">
			<Card.Root class="page-card">
				<Card.Header class="flex-row items-center justify-between space-y-0">
					<div>
						<Card.Title class="flex items-center gap-2">
							<HugeiconsIcon icon={Link01Icon} class="size-4" strokeWidth={2} />
							Invites
						</Card.Title>
						<Card.Description>One-time links for new peers</Card.Description>
					</div>
					<Button size="sm" variant="outline" onclick={createInvite}>
						<HugeiconsIcon icon={PlusSignIcon} class="size-4" strokeWidth={2} />
						Create
					</Button>
				</Card.Header>
				<Card.Content class="p-0">
					{#if fleetInvites.length === 0}
						<p class="p-4 text-sm text-muted-foreground">No active invites.</p>
					{:else}
						<Table.Root>
							<Table.Header>
								<Table.Row class="hover:bg-transparent">
									<Table.Head>Created</Table.Head>
									<Table.Head>Status</Table.Head>
									<Table.Head class="text-right">Actions</Table.Head>
								</Table.Row>
							</Table.Header>
							<Table.Body>
								{#each fleetInvites as invite (invite.id)}
									<Table.Row>
										<Table.Cell class="text-xs">{formatRelative(invite.created_at)}</Table.Cell>
										<Table.Cell>
											{#if invite.used_at}
												<Badge variant="secondary">used</Badge>
											{:else if invite.expires_at && new Date(invite.expires_at) < new Date()}
												<Badge variant="outline">expired</Badge>
											{:else}
												<Badge>active</Badge>
											{/if}
										</Table.Cell>
										<Table.Cell class="text-right">
											<div class="flex justify-end gap-1">
												<Button
													variant="ghost"
													size="sm"
													disabled={!invite.join_url}
													onclick={() =>
														copyText(
															invite.join_url || invite.token,
															invite.join_url ? 'Invite link' : 'Invite token'
														)}
												>
													<HugeiconsIcon icon={Copy01Icon} class="size-4" strokeWidth={2} />
													Copy
												</Button>
												<Button
													variant="ghost"
													size="sm"
													class="text-destructive"
													onclick={() => {
														deleteInviteTarget = invite;
														deleteInviteOpen = true;
													}}
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

			<Card.Root class="page-card">
				<Card.Header>
					<Card.Title>Join another panel</Card.Title>
					<Card.Description>Connect this instance to a remote PgPanel fleet</Card.Description>
				</Card.Header>
				<Card.Content>
					<form class="space-y-4" onsubmit={joinPanel}>
						<div class="space-y-2">
							<Label for="join-url">Base URL</Label>
							<Input
								id="join-url"
								bind:value={joinBaseUrl}
								placeholder="https://other-panel.example.com"
								required
								class="font-mono text-sm"
							/>
						</div>
						<div class="space-y-2">
							<Label for="join-token">Token</Label>
							<Input id="join-token" bind:value={joinToken} required class="font-mono text-sm" />
						</div>
						<div class="space-y-2">
							<Label for="join-name">Local name <span class="text-muted-foreground">(optional)</span></Label>
							<Input id="join-name" bind:value={joinLocalName} placeholder="This panel" />
						</div>
						<Button type="submit" disabled={joining} class="w-full">
							{joining ? 'Joining…' : 'Join panel'}
						</Button>
					</form>
				</Card.Content>
			</Card.Root>
		</div>

		<Card.Root class="page-card">
			<Card.Header>
				<Card.Title>Fleet peers</Card.Title>
				<Card.Description>Connected remote panels</Card.Description>
			</Card.Header>
			<Card.Content class="p-0">
				{#if fleetPeers.length === 0}
					<p class="p-4 text-sm text-muted-foreground">No peers connected yet.</p>
				{:else}
					<div class="table-scroll">
						<Table.Root>
							<Table.Header>
								<Table.Row class="hover:bg-transparent">
									<Table.Head>Name</Table.Head>
									<Table.Head>URL</Table.Head>
									<Table.Head>Status</Table.Head>
									<Table.Head>Capacity</Table.Head>
									<Table.Head class="text-right">Actions</Table.Head>
								</Table.Row>
							</Table.Header>
							<Table.Body>
								{#each fleetPeers as peer (peer.id)}
									<Table.Row>
										<Table.Cell class="font-medium">{peer.name}</Table.Cell>
										<Table.Cell class="max-w-[12rem] truncate font-mono text-xs">{peer.base_url}</Table.Cell>
										<Table.Cell>
											<StatusBadge status={peer.status} />
											{#if peer.last_ping_ms != null}
												<span class="ml-1 text-xs text-muted-foreground">{peer.last_ping_ms} ms</span>
											{/if}
										</Table.Cell>
										<Table.Cell class="tabular-nums text-xs">
											{#if peer.cluster_count != null}
												{formatNodeCapacity(peer.cluster_count, peer.max_clusters)}
											{:else}
												—
											{/if}
										</Table.Cell>
										<Table.Cell class="text-right">
											<div class="flex justify-end gap-1">
												<Button
													variant="ghost"
													size="sm"
													disabled={peerPinging === peer.id}
													onclick={() => pingPeer(peer.id)}
												>
													Ping
												</Button>
												<Button
													variant="ghost"
													size="sm"
													class="text-destructive"
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
	</Tabs.Content>
</Tabs.Root>

<!-- Create node dialog -->
<Dialog.Root bind:open={createOpen}>
	<Dialog.Content class="sm:max-w-lg">
		<Dialog.Header>
			<Dialog.Title class="flex items-center gap-2">
				<HugeiconsIcon icon={CloudServerIcon} class="size-5 text-primary" strokeWidth={2} />
				Add remote node
			</Dialog.Title>
			<Dialog.Description>
				Connect a remote Docker host via TCP. The panel probes connectivity before saving.
			</Dialog.Description>
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
				<Label for="create-max">Max clusters <span class="text-muted-foreground">(empty = unlimited)</span></Label>
				<Input id="create-max" type="number" min="1" bind:value={createMaxClusters} placeholder="Unlimited" />
			</div>
			<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
				<span>Set as default node for new clusters</span>
				<Switch bind:checked={createSetDefault} />
			</label>
			<Dialog.Footer class="gap-2 border-t-0 bg-transparent sm:justify-end">
				<Button type="button" variant="outline" onclick={() => (createOpen = false)}>Cancel</Button>
				<Button type="submit" disabled={creating}>
					{creating ? 'Adding…' : 'Add node'}
				</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>

<!-- Edit node dialog -->
<Dialog.Root bind:open={editOpen}>
	<Dialog.Content class="sm:max-w-lg">
		<Dialog.Header>
			<Dialog.Title>Edit node</Dialog.Title>
			<Dialog.Description>
				{#if editNode?.kind === 'local'}
					Local node — docker host cannot be changed here.
				{:else}
					Update remote node settings.
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
					<Label for="edit-max">Max clusters <span class="text-muted-foreground">(empty = unlimited)</span></Label>
					<Input id="edit-max" type="number" min="1" bind:value={editMaxClusters} placeholder="Unlimited" />
				</div>
				<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
					<span>Default node for new clusters</span>
					<Switch bind:checked={editSetDefault} />
				</label>
				<Dialog.Footer class="gap-2 border-t-0 bg-transparent sm:justify-end">
					<Button type="button" variant="outline" onclick={() => (editOpen = false)}>Cancel</Button>
					<Button type="submit" disabled={editing}>{editing ? 'Saving…' : 'Save changes'}</Button>
				</Dialog.Footer>
			</form>
		{/if}
	</Dialog.Content>
</Dialog.Root>

<ConfirmDialog
	bind:open={deleteOpen}
	title="Delete remote node?"
	description={deleteTarget ? `Delete "${deleteTarget.name}"? This cannot be undone.` : ''}
	confirmLabel="Delete"
	variant="destructive"
	onConfirm={confirmDelete}
/>

<ConfirmDialog
	bind:open={rotateConfirmOpen}
	title="Rotate join token?"
	description="Existing join links will stop working. Peers already connected are unaffected."
	confirmLabel="Rotate"
	variant="destructive"
	onConfirm={rotateToken}
/>

<ConfirmDialog
	bind:open={deleteInviteOpen}
	title="Delete invite?"
	description="This invite link will no longer work."
	confirmLabel="Delete"
	variant="destructive"
	onConfirm={deleteInvite}
/>

<ConfirmDialog
	bind:open={deletePeerOpen}
	title="Remove peer?"
	description={deletePeerTarget ? `Disconnect from "${deletePeerTarget.name}"?` : ''}
	confirmLabel="Remove"
	variant="destructive"
	onConfirm={removePeer}
/>
