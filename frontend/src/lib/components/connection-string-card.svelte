<script lang="ts">
	import {
		api,
		type ClusterConnectionInfo,
		type ClusterConnectionRevealResponse
	} from '$lib/api';
	import SearchableSelect, { type SearchableOption } from '$lib/components/searchable-select.svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Input } from '$lib/components/ui/input/index.js';
	import { Label } from '$lib/components/ui/label/index.js';
	import * as Card from '$lib/components/ui/card/index.js';
	import * as Alert from '$lib/components/ui/alert/index.js';
	import * as Select from '$lib/components/ui/select/index.js';
	import { toast } from 'svelte-sonner';
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import {
		Alert02Icon,
		Copy01Icon,
		InformationCircleIcon,
		RefreshIcon
	} from '@hugeicons/core-free-icons';

	let { clusterId }: { clusterId: string } = $props();

	let connection = $state<ClusterConnectionInfo | null>(null);
	let loading = $state(false);
	let loadError = $state('');
	let revealing = $state(false);

	let selectedRole = $state('');
	let selectedDatabase = $state('');
	let hostMode = $state<'internal' | 'public'>('internal');

	let revealed = $state<ClusterConnectionRevealResponse | null>(null);

	const roleItems = $derived<SearchableOption[]>(
		(connection?.roles ?? []).map((role) => ({ value: role, label: role }))
	);

	const databaseItems = $derived<SearchableOption[]>(
		(connection?.databases ?? []).map((db) => ({
			value: db.name,
			label: db.name,
			description: db.owner_role ? `owner: ${db.owner_role}` : undefined
		}))
	);

	const publicAvailable = $derived(connection?.public_port != null);

	const displayHost = $derived(connection?.internal_host ?? '');

	const displayPort = $derived(
		connection
			? hostMode === 'public' && connection.public_port != null
				? connection.public_port
				: connection.default_port
			: 5432
	);

	const uriTemplate = $derived(
		selectedRole && selectedDatabase && displayHost
			? `postgresql://${selectedRole}@${displayHost}:${displayPort}/${selectedDatabase}`
			: ''
	);

	async function loadConnection(id: string) {
		if (!id) {
			connection = null;
			return;
		}
		loading = true;
		loadError = '';
		try {
			const info = await api<ClusterConnectionInfo>(`/api/clusters/${id}/connection`);
			connection = info;
			selectedRole = info.default_role;
			selectedDatabase = info.default_database;
			hostMode = 'internal';
			revealed = null;
		} catch (e) {
			loadError = e instanceof Error ? e.message : 'Failed to load connection info';
			connection = null;
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		void loadConnection(clusterId);
	});

	function resetReveal() {
		revealed = null;
	}

	function onRoleChange() {
		resetReveal();
	}

	function onDatabaseChange(value: string) {
		const owner = connection?.databases.find((db) => db.name === value)?.owner_role;
		if (owner && connection?.roles.includes(owner)) {
			selectedRole = owner;
		}
		resetReveal();
	}

	function onHostModeChange(value: string | undefined) {
		if (value === 'internal' || value === 'public') {
			hostMode = value;
			resetReveal();
		}
	}

	async function copyText(text: string) {
		await navigator.clipboard.writeText(text);
		toast.success('Copied to clipboard');
	}

	async function revealAndCopy() {
		if (!connection || !selectedRole || !selectedDatabase) return;
		revealing = true;
		loadError = '';
		try {
			const result = await api<ClusterConnectionRevealResponse>(
				`/api/clusters/${clusterId}/connection/reveal`,
				{
					method: 'POST',
					body: JSON.stringify({
						role: selectedRole,
						database: selectedDatabase,
						host_mode: hostMode
					})
				}
			);
			revealed = result;
			await copyText(result.connection_string);
		} catch (e) {
			const message = e instanceof Error ? e.message : 'Failed to reveal password';
			loadError = message;
			toast.error(message);
		} finally {
			revealing = false;
		}
	}

	const hostModeLabel = $derived(
		hostMode === 'public' ? 'Public' : 'Internal'
	);
</script>

<Card.Root class="border-border/60">
	<Card.Header class="flex-row items-center justify-between gap-4">
		<div>
			<Card.Title>Connection string</Card.Title>
			<Card.Description>
				Build a PostgreSQL URI for this cluster. Password is revealed on demand.
			</Card.Description>
		</div>
		<Button
			variant="outline"
			size="sm"
			disabled={loading || !clusterId}
			onclick={() => loadConnection(clusterId)}
		>
			<HugeiconsIcon icon={RefreshIcon} class="size-4" strokeWidth={2} />
			Refresh
		</Button>
	</Card.Header>
	<Card.Content class="space-y-4">
		{#if loadError}
			<Alert.Root variant="destructive">
				<HugeiconsIcon icon={Alert02Icon} class="size-4" strokeWidth={2} />
				<Alert.Description>{loadError}</Alert.Description>
			</Alert.Root>
		{/if}

		{#if loading && !connection}
			<p class="text-sm text-muted-foreground">Loading connection info…</p>
		{:else if connection}
			<div class="grid gap-4 sm:grid-cols-3">
				<div class="space-y-1.5">
					<Label for="conn-database">Database</Label>
					<SearchableSelect
						id="conn-database"
						bind:value={selectedDatabase}
						items={databaseItems}
						placeholder="Select database…"
						mono
						onValueChange={onDatabaseChange}
					/>
				</div>
				<div class="space-y-1.5">
					<Label for="conn-role">Role</Label>
					<SearchableSelect
						id="conn-role"
						bind:value={selectedRole}
						items={roleItems}
						placeholder="Select role…"
						mono
						onValueChange={onRoleChange}
					/>
				</div>
				<div class="space-y-1.5">
					<Label>Host mode</Label>
					<Select.Root type="single" value={hostMode} onValueChange={onHostModeChange}>
						<Select.Trigger class="w-full">{hostModeLabel}</Select.Trigger>
						<Select.Content>
							<Select.Item value="internal" label="Internal">Internal</Select.Item>
							<Select.Item value="public" label="Public" disabled={!publicAvailable}>
								Public
								{#if !publicAvailable}
									<span class="text-muted-foreground"> (not exposed)</span>
								{/if}
							</Select.Item>
						</Select.Content>
					</Select.Root>
				</div>
			</div>

			<div class="space-y-1.5">
				<Label>URI (no password)</Label>
				<Input
					readonly
					value={uriTemplate}
					class="font-mono text-xs"
					placeholder="Select database and role…"
				/>
			</div>

			<div class="flex flex-wrap items-center gap-2">
				<Button
					disabled={revealing || !selectedRole || !selectedDatabase}
					onclick={revealAndCopy}
				>
					{revealing ? 'Revealing…' : 'Reveal password & copy'}
				</Button>
			</div>

			{#if revealed}
				<Alert.Root class="border-amber-500/40 bg-amber-500/10">
					<HugeiconsIcon icon={InformationCircleIcon} class="size-4" strokeWidth={2} />
					<Alert.Title>Credentials revealed</Alert.Title>
					<Alert.Description class="mt-2 space-y-3">
						{#if revealed.warning}
							<p class="text-sm">{revealed.warning}</p>
						{/if}
						<div class="flex flex-wrap items-center gap-2">
							<Input
								readonly
								value={revealed.connection_string}
								class="min-w-0 flex-1 font-mono text-xs"
							/>
							<Button
								variant="secondary"
								size="sm"
								onclick={() => copyText(revealed!.connection_string)}
							>
								<HugeiconsIcon icon={Copy01Icon} class="size-4" strokeWidth={2} />
								Copy
							</Button>
						</div>
					</Alert.Description>
				</Alert.Root>
			{/if}
		{:else if !loading}
			<p class="text-sm text-muted-foreground">No connection info available.</p>
		{/if}
	</Card.Content>
</Card.Root>
