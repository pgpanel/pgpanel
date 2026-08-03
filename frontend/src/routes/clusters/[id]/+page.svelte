<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/stores';
	import { api, type Cluster } from '$lib/api';
	import { trackOperation } from '$lib/jobs';
	import PageHeader from '$lib/components/page-header.svelte';
	import StatusBadge from '$lib/components/status-badge.svelte';
	import ConnectionStringCard from '$lib/components/connection-string-card.svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Input } from '$lib/components/ui/input/index.js';
	import { Label } from '$lib/components/ui/label/index.js';
	import { Switch } from '$lib/components/ui/switch/index.js';
	import { Checkbox } from '$lib/components/ui/checkbox/index.js';
	import * as Card from '$lib/components/ui/card/index.js';
	import * as Alert from '$lib/components/ui/alert/index.js';
	import * as Dialog from '$lib/components/ui/dialog/index.js';
	import { Separator } from '$lib/components/ui/separator/index.js';
	import { toast } from 'svelte-sonner';
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import {
		PlayIcon,
		StopIcon,
		RefreshIcon,
		Alert02Icon,
		Delete02Icon
	} from '@hugeicons/core-free-icons';

	type DeleteMode = 'remove_from_panel' | 'delete_container_keep_volume' | 'permanently_delete';

	let cluster = $state<Cluster | null>(null);
	let error = $state('');
	let busy = $state('');

	let exposePublicly = $state(false);
	let publicPort = $state(5433);
	let networkingSaving = $state(false);

	let deleteDialogOpen = $state(false);
	let deleteStep = $state(1);
	let deleteMode = $state<DeleteMode>('remove_from_panel');
	let confirmName = $state('');
	let confirmVolumeDelete = $state(false);
	let deleteBusy = $state(false);

	const id = $derived($page.params.id);

	const deleteModes: { value: DeleteMode; label: string; description: string }[] = [
		{
			value: 'remove_from_panel',
			label: 'Remove from panel',
			description: 'Unregister cluster only — Docker resources stay untouched.'
		},
		{
			value: 'delete_container_keep_volume',
			label: 'Delete container, keep volume',
			description: 'Stop and remove container and network; data volume is preserved.'
		},
		{
			value: 'permanently_delete',
			label: 'Permanently delete',
			description: 'Destroy container, network, and data volume. Cannot be undone.'
		}
	];

	const nameMatches = $derived(cluster !== null && confirmName === cluster.name);
	const canConfirmDelete = $derived(
		nameMatches &&
			(deleteMode !== 'permanently_delete' || (confirmVolumeDelete && !cluster?.delete_protection))
	);

	async function load() {
		cluster = await api<Cluster>(`/api/clusters/${id}`);
		exposePublicly = cluster.public_port != null;
		publicPort = cluster.public_port ?? 5433;
	}

	onMount(() => {
		load().catch((e) => (error = e.message));
		const t = setInterval(() => load().catch(() => {}), 5000);
		return () => clearInterval(t);
	});

	async function action(path: string, label: string) {
		busy = label;
		error = '';
		try {
			await api(`/api/clusters/${id}/${path}`, { method: 'POST' });
			toast.success(`${label} queued`);
			await load();
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed';
			toast.error(error);
		} finally {
			busy = '';
		}
	}

	async function saveNetworking() {
		networkingSaving = true;
		error = '';
		try {
			const res = await api<{ operation_id: string; message: string }>(
				`/api/clusters/${id}/networking`,
				{
					method: 'PUT',
					body: JSON.stringify({
						expose_publicly: exposePublicly,
						public_port: exposePublicly ? publicPort : null
					})
				}
			);
			trackOperation(res.operation_id, {
				title: 'Update networking',
				onDone: () => load(),
				onFail: () => load()
			});
			toast.success(res.message || 'Networking update queued');
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed';
			toast.error(error);
		} finally {
			networkingSaving = false;
		}
	}

	function openDeleteDialog() {
		deleteStep = 1;
		deleteMode = 'remove_from_panel';
		confirmName = '';
		confirmVolumeDelete = false;
		deleteDialogOpen = true;
	}

	function nextDeleteStep() {
		if (deleteStep < (deleteMode === 'permanently_delete' ? 3 : 2)) {
			deleteStep += 1;
		}
	}

	function prevDeleteStep() {
		if (deleteStep > 1) deleteStep -= 1;
	}

	async function confirmDelete() {
		if (!cluster || !canConfirmDelete) return;
		deleteBusy = true;
		try {
			const res = await api<{ operation_id: string }>(`/api/clusters/${id}`, {
				method: 'DELETE',
				body: JSON.stringify({
					mode: deleteMode,
					confirm_name: confirmName,
					confirm_volume_delete: deleteMode === 'permanently_delete' ? confirmVolumeDelete : false
				})
			});
			deleteDialogOpen = false;
			trackOperation(res.operation_id, {
				title: `Delete cluster “${cluster.name}”`,
				onDone: () => goto('/clusters')
			});
			toast.success('Cluster deletion queued');
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Delete failed');
		} finally {
			deleteBusy = false;
		}
	}
</script>

{#if cluster}
	<PageHeader title={cluster.name} description={`${cluster.slug} · PostgreSQL ${cluster.postgres_version}`}>
		{#snippet actions()}
			<StatusBadge status={cluster?.status ?? 'unknown'} />
			<Button variant="outline" size="sm" disabled={!!busy} onclick={() => action('start', 'Start')}>
				<HugeiconsIcon icon={PlayIcon} class="size-4" strokeWidth={2} />
				Start
			</Button>
			<Button variant="outline" size="sm" disabled={!!busy} onclick={() => action('stop', 'Stop')}>
				<HugeiconsIcon icon={StopIcon} class="size-4" strokeWidth={2} />
				Stop
			</Button>
			<Button variant="outline" size="sm" disabled={!!busy} onclick={() => action('restart', 'Restart')}>
				<HugeiconsIcon icon={RefreshIcon} class="size-4" strokeWidth={2} />
				Restart
			</Button>
			<Button variant="destructive" size="sm" onclick={openDeleteDialog}>
				<HugeiconsIcon icon={Delete02Icon} class="size-4" strokeWidth={2} />
				Delete cluster
			</Button>
		{/snippet}
	</PageHeader>

	{#if error}
		<Alert.Root variant="destructive" class="mb-4">
			<HugeiconsIcon icon={Alert02Icon} class="size-4" strokeWidth={2} />
			<Alert.Description>{error}</Alert.Description>
		</Alert.Root>
	{/if}

	<div class="grid gap-4 md:grid-cols-2">
		<ConnectionStringCard clusterId={id} />

		<Card.Root class="border-border/60">
			<Card.Header>
				<Card.Title>Resources</Card.Title>
				<Card.Description>Container limits and platform status</Card.Description>
			</Card.Header>
			<Card.Content class="space-y-3 text-sm">
				<div class="flex justify-between gap-4">
					<span class="text-muted-foreground">CPU / Memory</span>
					<span>{cluster.cpu_limit} / {cluster.memory_mb} MB</span>
				</div>
				<Separator />
				<div class="flex justify-between gap-4">
					<span class="text-muted-foreground">Storage limit</span>
					<span>{cluster.storage_limit_gb} GB</span>
				</div>
				<Separator />
				<div class="flex justify-between gap-4">
					<span class="text-muted-foreground">Native backups</span>
					<span>{cluster.enable_backup ? 'on' : 'off'}</span>
				</div>
				<Separator />
				<div class="flex justify-between gap-4">
					<span class="text-muted-foreground">Delete protection</span>
					<span>{cluster.delete_protection ? 'on' : 'off'}</span>
				</div>
			</Card.Content>
		</Card.Root>

		<Card.Root class="border-border/60">
			<Card.Header>
				<Card.Title>Public port</Card.Title>
				<Card.Description>Expose PostgreSQL on the Docker host (container will be recreated)</Card.Description>
			</Card.Header>
			<Card.Content class="space-y-4">
				<div class="flex items-center justify-between gap-4">
					<Label>Expose publicly</Label>
					<Switch bind:checked={exposePublicly} />
				</div>
				{#if exposePublicly}
					<div class="space-y-2">
						<Label for="public-port">Port (≥ 1024)</Label>
						<Input id="public-port" type="number" min="1024" bind:value={publicPort} />
					</div>
				{/if}
			</Card.Content>
			<Card.Footer class="justify-end border-t">
				<Button onclick={saveNetworking} disabled={networkingSaving}>
					{networkingSaving ? 'Saving…' : 'Save networking'}
				</Button>
			</Card.Footer>
		</Card.Root>

		{#if cluster.last_error}
			<Card.Root class="border-destructive/40 bg-destructive/5">
				<Card.Header>
					<Card.Title class="text-destructive">Last error</Card.Title>
				</Card.Header>
				<Card.Content>
					<p class="text-sm text-destructive/90">{cluster.last_error}</p>
				</Card.Content>
			</Card.Root>
		{:else}
			<Card.Root class="border-border/60">
				<Card.Header>
					<Card.Title>Health</Card.Title>
					<Card.Description>Auto-refreshes every 5 seconds</Card.Description>
				</Card.Header>
				<Card.Content class="flex items-center gap-3">
					<StatusBadge status={cluster.health} />
					<span class="text-sm text-muted-foreground">container health status</span>
				</Card.Content>
			</Card.Root>
		{/if}
	</div>
{:else}
	<div class="text-muted-foreground">Loading cluster…</div>
{/if}

<Dialog.Root bind:open={deleteDialogOpen}>
	<Dialog.Content class="sm:max-w-lg">
		<Dialog.Header>
			<Dialog.Title>Delete cluster</Dialog.Title>
			<Dialog.Description>
				{#if deleteStep === 1}
					Choose how to remove this cluster.
				{:else if deleteStep === 2}
					Type the cluster name to confirm.
				{:else}
					Final confirmation for permanent deletion.
				{/if}
			</Dialog.Description>
		</Dialog.Header>

		{#if deleteStep === 1}
			<div class="space-y-2 py-2">
				{#each deleteModes as mode (mode.value)}
					<label
						class="flex cursor-pointer gap-3 rounded-lg border p-3 transition-colors {deleteMode ===
						mode.value
							? 'border-primary bg-primary/5'
							: 'border-border/60 hover:bg-muted/30'}"
					>
						<input
							type="radio"
							name="delete-mode"
							value={mode.value}
							bind:group={deleteMode}
							class="mt-1"
							disabled={mode.value === 'permanently_delete' && cluster?.delete_protection}
						/>
						<div class="space-y-0.5">
							<p class="text-sm font-medium">{mode.label}</p>
							<p class="text-xs text-muted-foreground">{mode.description}</p>
							{#if mode.value === 'permanently_delete' && cluster?.delete_protection}
								<p class="text-xs text-destructive">
									Delete protection is enabled — disable it before permanent delete.
								</p>
							{/if}
						</div>
					</label>
				{/each}
			</div>
		{:else if deleteStep === 2}
			<div class="space-y-2 py-2">
				<Label for="confirm-name">
					Type <span class="font-mono font-medium">{cluster?.name}</span> to confirm
				</Label>
				<Input
					id="confirm-name"
					bind:value={confirmName}
					placeholder={cluster?.name}
					autocomplete="off"
				/>
			</div>
		{:else}
			<div class="space-y-4 py-2">
				<Alert.Root variant="destructive">
					<HugeiconsIcon icon={Alert02Icon} class="size-4" strokeWidth={2} />
					<Alert.Description>
						This will permanently destroy the container, network, and data volume for
						<strong>{cluster?.name}</strong>.
					</Alert.Description>
				</Alert.Root>
				<label class="flex items-start gap-3 text-sm">
					<Checkbox bind:checked={confirmVolumeDelete} class="mt-0.5" />
					<span>I understand the volume will be wiped and this cannot be undone.</span>
				</label>
			</div>
		{/if}

		<Dialog.Footer class="gap-2 border-t-0 bg-transparent sm:justify-between">
			<div>
				{#if deleteStep > 1}
					<Button variant="outline" onclick={prevDeleteStep} disabled={deleteBusy}>Back</Button>
				{/if}
			</div>
			<div class="flex gap-2">
				<Button variant="outline" onclick={() => (deleteDialogOpen = false)} disabled={deleteBusy}>
					Cancel
				</Button>
				{#if deleteStep === 1}
					<Button
						variant="destructive"
						onclick={nextDeleteStep}
						disabled={deleteMode === 'permanently_delete' && cluster?.delete_protection}
					>
						Continue
					</Button>
				{:else if deleteStep === 2}
					{#if deleteMode === 'permanently_delete'}
						<Button variant="destructive" onclick={nextDeleteStep} disabled={!nameMatches}>
							Continue
						</Button>
					{:else}
						<Button
							variant="destructive"
							onclick={confirmDelete}
							disabled={!canConfirmDelete || deleteBusy}
						>
							{deleteBusy ? 'Deleting…' : 'Delete cluster'}
						</Button>
					{/if}
				{:else}
					<Button
						variant="destructive"
						onclick={confirmDelete}
						disabled={!canConfirmDelete || deleteBusy}
					>
						{deleteBusy ? 'Deleting…' : 'Delete permanently'}
					</Button>
				{/if}
			</div>
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>
