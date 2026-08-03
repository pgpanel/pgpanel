<script lang="ts">
	import { onMount } from 'svelte';
	import {
		api,
		type GlobalBackupPolicy,
		type ApiTokenInfo,
		type ApiTokenCreated,
		type UpdateStatus
	} from '$lib/api';
	import {
		fetchUpdateStatus,
		checkForUpdates,
		applyUpdate,
		waitForPanelRecovery,
		updateChecking,
		updateError
	} from '$lib/updates';
	import ConfirmDialog from '$lib/components/confirm-dialog.svelte';
	import PageHeader from '$lib/components/page-header.svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Input } from '$lib/components/ui/input/index.js';
	import { Label } from '$lib/components/ui/label/index.js';
	import { Switch } from '$lib/components/ui/switch/index.js';
	import * as Select from '$lib/components/ui/select/index.js';
	import { toast } from 'svelte-sonner';
	import * as Card from '$lib/components/ui/card/index.js';
	import * as Alert from '$lib/components/ui/alert/index.js';
	import { Badge } from '$lib/components/ui/badge/index.js';
	import { Separator } from '$lib/components/ui/separator/index.js';
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import {
		CheckmarkCircle02Icon,
		Alert02Icon,
		CloudUploadIcon,
		RefreshIcon,
		CloudBackupIcon,
		Key01Icon,
		Delete02Icon,
		Copy01Icon
	} from '@hugeicons/core-free-icons';

	let health = $state<{ status: string; version: string } | null>(null);
	let ready = $state<{ ready: boolean; database: boolean; docker: boolean } | null>(null);
	let storage = $state({
		storage_type: 'local',
		endpoint: '',
		region: 'auto',
		bucket: '',
		prefix: 'pgpanel/',
		path_style: true,
		tls_verify: true,
		encrypt: true,
		access_key: '',
		secret_key: '',
		access_key_set: false,
		secret_key_set: false,
		retention_days: 14,
		keep_count: 30,
		schedule_hour: 3
	});
	let storageSaving = $state(false);
	let storageMessage = $state('');
	let storageError = $state('');

	let backupPolicy = $state<GlobalBackupPolicy>({
		retention_days: 14,
		keep_count: 30,
		schedule_hour: 3,
		cron_default: '0 3 * * *',
		compression_level: 6,
		dump_format: 'custom',
		verify_after: false,
		keep_local_copy: false,
		notify_webhook: '',
		notify_on_success: false,
		notify_on_failure: true,
		exclude_schemas_default: '',
		wal_archiving_default: false,
		parallel_jobs: 1,
		encrypt: true
	});
	let backupSaving = $state(false);
	let backupMessage = $state('');
	let backupError = $state('');

	let tokens = $state<ApiTokenInfo[]>([]);
	let tokenName = $state('');
	let tokenRole = $state('operator');
	let tokenScopes = $state('read,write');
	let tokenExpiresDays = $state<number | ''>('');
	let tokenCreating = $state(false);
	let createdToken = $state<ApiTokenCreated | null>(null);

	let updates = $state<UpdateStatus | null>(null);
	let applyOpen = $state(false);
	let applying = $state(false);
	let updateOverlay = $state(false);
	let revokeTokenId = $state<string | null>(null);
	let revokeOpen = $state(false);

	onMount(async () => {
		try {
			[health, ready, storage, backupPolicy, tokens] = await Promise.all([
				api<{ status: string; version: string }>('/health'),
				api<{ ready: boolean; database: boolean; docker: boolean }>('/ready'),
				api<typeof storage>('/api/settings/storage'),
				api<GlobalBackupPolicy>('/api/settings/backup-policy'),
				api<ApiTokenInfo[]>('/api/tokens').catch(() => [] as ApiTokenInfo[])
			]);
		} catch {
			storageError = 'Could not load the current storage configuration';
		}
		updates = await fetchUpdateStatus();
		if (window.location.hash === '#updates') {
			requestAnimationFrame(() => {
				document.getElementById('updates')?.scrollIntoView({ behavior: 'smooth', block: 'start' });
			});
		}
	});

	async function loadTokens() {
		tokens = await api<ApiTokenInfo[]>('/api/tokens');
	}

	async function createToken(e: Event) {
		e.preventDefault();
		tokenCreating = true;
		createdToken = null;
		try {
			createdToken = await api<ApiTokenCreated>('/api/tokens', {
				method: 'POST',
				body: JSON.stringify({
					name: tokenName,
					role: tokenRole,
					scopes: tokenScopes,
					expires_days: tokenExpiresDays === '' ? null : tokenExpiresDays
				})
			});
			toast.success('API token created — copy it now');
			tokenName = '';
			tokenExpiresDays = '';
			await loadTokens();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Token creation failed');
		} finally {
			tokenCreating = false;
		}
	}

	function askRevokeToken(id: string) {
		revokeTokenId = id;
		revokeOpen = true;
	}

	async function revokeToken() {
		if (!revokeTokenId) return;
		try {
			await api(`/api/tokens/${revokeTokenId}`, { method: 'DELETE' });
			toast.success('Token revoked');
			await loadTokens();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Revoke failed');
		} finally {
			revokeTokenId = null;
		}
	}

	async function handleCheckUpdates() {
		updates = await checkForUpdates();
		if (updates?.update_available) {
			toast.success(`Update available: ${updates.latest_version}`);
		} else {
			toast.success('You are on the latest version');
		}
	}

	async function handleApplyUpdate() {
		applying = true;
		try {
			const result = await applyUpdate();

			if (result.status === 'up_to_date') {
				toast.success(result.message ?? 'Already on the latest version');
				return;
			}

			if (result.status === 'started' || result.poll_health) {
				updateOverlay = true;
				const recovered = await waitForPanelRecovery();
				if (recovered) {
					toast.success('Panel updated — reconnected successfully');
					updates = await fetchUpdateStatus();
					updateOverlay = false;
					window.location.reload();
				} else {
					updateOverlay = false;
					toast.error(
						'Panel did not come back in time. Refresh this page manually or run: sudo pgpanel update'
					);
				}
			} else {
				toast.success(result.message ?? 'Update started — the panel may restart shortly');
				setTimeout(() => fetchUpdateStatus(), 3000);
			}
		} catch (e) {
			updateOverlay = false;
			toast.error(e instanceof Error ? e.message : 'Update failed');
		} finally {
			applying = false;
		}
	}

	async function copyToken() {
		if (!createdToken) return;
		try {
			await navigator.clipboard.writeText(createdToken.token);
			toast.success('Token copied to clipboard');
		} catch {
			toast.error('Could not copy — select and copy manually');
		}
	}

	async function saveBackupPolicy() {
		backupSaving = true;
		backupMessage = '';
		backupError = '';
		try {
			backupPolicy = await api<GlobalBackupPolicy>('/api/settings/backup-policy', {
				method: 'POST',
				body: JSON.stringify(backupPolicy)
			});
			backupMessage = 'Backup policy saved.';
		} catch (e) {
			backupError = e instanceof Error ? e.message : 'Backup policy save failed';
		} finally {
			backupSaving = false;
		}
	}

	async function saveStorage() {
		storageSaving = true;
		storageMessage = '';
		storageError = '';
		try {
			const saved = await api<typeof storage>('/api/settings/storage', {
				method: 'POST',
				body: JSON.stringify(storage)
			});
			storage = { ...storage, ...saved, access_key: '', secret_key: '' };
			storageMessage = 'Storage saved and connection verified.';
		} catch (e) {
			storageError = e instanceof Error ? e.message : 'Storage configuration failed';
		} finally {
			storageSaving = false;
		}
	}
</script>

<PageHeader title="Settings" description="Storage, backup policy, updates, health and security posture" />

<Card.Root id="updates" class="mb-6 page-card">
	<Card.Header class="flex-row items-start justify-between gap-4">
		<div>
			<Card.Title class="flex items-center gap-2">
				<HugeiconsIcon icon={RefreshIcon} class="size-5 text-primary" strokeWidth={2} />
				Panel updates
				{#if updates?.update_available}
					<Badge class="bg-amber-500/15 text-amber-300 ring-1 ring-amber-500/30">Update available</Badge>
				{/if}
			</Card.Title>
			<Card.Description>Check for and apply in-place panel updates</Card.Description>
		</div>
	</Card.Header>
	<Card.Content class="space-y-4 text-sm">
		{#if $updateError}
			<Alert.Root variant="destructive">
				<HugeiconsIcon icon={Alert02Icon} class="size-4" strokeWidth={2} />
				<Alert.Description>{$updateError}</Alert.Description>
			</Alert.Root>
		{/if}
		<div class="grid gap-3 sm:grid-cols-2">
			<div class="flex items-center justify-between rounded-lg border border-border/60 p-3">
				<span class="text-muted-foreground">Current version</span>
				<span class="font-mono text-xs">{updates?.current_version ?? health?.version ?? '—'}</span>
			</div>
			<div class="flex items-center justify-between rounded-lg border border-border/60 p-3">
				<span class="text-muted-foreground">Latest version</span>
				<span class="font-mono text-xs">{updates?.latest_version ?? '—'}</span>
			</div>
		</div>
		{#if updates?.last_checked_at}
			<p class="text-xs text-muted-foreground">
				Last checked: {new Date(updates.last_checked_at).toLocaleString()}
			</p>
		{/if}
		{#if updates?.changelog}
			<p class="rounded-lg border border-border/60 bg-muted/20 p-3 text-xs text-muted-foreground">
				{updates.changelog}
			</p>
		{/if}
		<div class="flex flex-wrap gap-2">
			<Button variant="outline" onclick={handleCheckUpdates} disabled={$updateChecking}>
				{$updateChecking ? 'Checking…' : 'Check for updates'}
			</Button>
			{#if updates?.update_available}
				<Button onclick={() => (applyOpen = true)} disabled={applying}>
					{applying ? 'Updating…' : 'Update now'}
				</Button>
			{/if}
		</div>
	</Card.Content>
</Card.Root>

{#if storageError}
	<Alert.Root variant="destructive" class="mb-4">
		<HugeiconsIcon icon={Alert02Icon} class="size-4" strokeWidth={2} />
		<Alert.Description>{storageError}</Alert.Description>
	</Alert.Root>
{/if}

<Card.Root class="mb-6 border-border/60">
	<Card.Header class="flex-row items-start justify-between gap-4">
		<div>
			<Card.Title class="flex items-center gap-2">
				<HugeiconsIcon icon={CloudUploadIcon} class="size-5 text-primary" strokeWidth={2} />
				Storage manager
			</Card.Title>
			<Card.Description>Configure local disk, AWS S3, R2, B2, Hetzner or MinIO without editing .env.</Card.Description>
		</div>
		{#if storageMessage}
			<Badge variant="default">{storageMessage}</Badge>
		{/if}
	</Card.Header>
	<Card.Content class="space-y-6">
		<div class="grid gap-4 md:grid-cols-2">
			<div class="space-y-2">
				<Label>Storage type</Label>
				<Select.Root type="single" bind:value={storage.storage_type}>
					<Select.Trigger class="w-full">{storage.storage_type === 'local' ? 'Local disk' : storage.storage_type.toUpperCase()}</Select.Trigger>
					<Select.Content>
						<Select.Item value="local" label="Local disk">Local disk</Select.Item>
						<Select.Item value="s3" label="AWS S3 / S3-compatible">AWS S3 / S3-compatible</Select.Item>
						<Select.Item value="r2" label="Cloudflare R2">Cloudflare R2</Select.Item>
						<Select.Item value="b2" label="Backblaze B2">Backblaze B2</Select.Item>
						<Select.Item value="hetzner" label="Hetzner Object Storage">Hetzner Object Storage</Select.Item>
						<Select.Item value="minio" label="MinIO">MinIO</Select.Item>
					</Select.Content>
				</Select.Root>
			</div>
			<div class="space-y-2">
				<Label>Bucket</Label>
				<Input bind:value={storage.bucket} placeholder="production-backups" disabled={storage.storage_type === 'local'} />
			</div>
		</div>

		{#if storage.storage_type !== 'local'}
			<div class="grid gap-4 md:grid-cols-2">
				<div class="space-y-2 md:col-span-2">
					<Label>Endpoint URL <span class="text-muted-foreground">(optional for AWS)</span></Label>
					<Input bind:value={storage.endpoint} placeholder="https://s3.eu-central-1.amazonaws.com" />
				</div>
				<div class="space-y-2">
					<Label>Region</Label>
					<Input bind:value={storage.region} placeholder="auto" />
				</div>
				<div class="space-y-2">
					<Label>Object prefix</Label>
					<Input bind:value={storage.prefix} placeholder="pgpanel/" />
				</div>
				<div class="space-y-2">
					<Label>Access key {#if storage.access_key_set}<span class="text-xs text-emerald-400">(saved)</span>{/if}</Label>
					<Input bind:value={storage.access_key} autocomplete="off" placeholder={storage.access_key_set ? 'Leave blank to keep current key' : 'Access key'} />
				</div>
				<div class="space-y-2">
					<Label>Secret key {#if storage.secret_key_set}<span class="text-xs text-emerald-400">(saved)</span>{/if}</Label>
					<Input type="password" bind:value={storage.secret_key} autocomplete="new-password" placeholder={storage.secret_key_set ? 'Leave blank to keep current key' : 'Secret key'} />
				</div>
			</div>
			<div class="grid gap-3 sm:grid-cols-3">
				<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
					<span>Path-style addressing</span>
					<Switch bind:checked={storage.path_style} />
				</label>
				<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
					<span>Verify TLS</span>
					<Switch bind:checked={storage.tls_verify} />
				</label>
				<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
					<span>Encrypt backups</span>
					<Switch bind:checked={storage.encrypt} />
				</label>
			</div>
		{:else}
			<div class="rounded-lg border border-border/60 bg-muted/20 p-4 text-sm text-muted-foreground">
				Backups are stored under the panel data directory. Switch to an object store when you need off-host durability.
			</div>
		{/if}

		<div class="grid gap-4 border-t border-border/60 pt-5 sm:grid-cols-2">
			<div class="space-y-2">
				<Label>Retention (days)</Label>
				<Input type="number" min="1" max="3650" bind:value={storage.retention_days} />
			</div>
			<div class="space-y-2">
				<Label>Keep at most</Label>
				<Input type="number" min="1" max="10000" bind:value={storage.keep_count} />
			</div>
			<div class="space-y-2 sm:col-span-2">
				<Label>Daily backup time (UTC)</Label>
				<Input type="number" min="0" max="23" bind:value={storage.schedule_hour} />
				<p class="text-xs text-muted-foreground">New schedules run once per day at this UTC hour.</p>
			</div>
		</div>
	</Card.Content>
	<Card.Footer class="justify-end">
		<Button onclick={saveStorage} disabled={storageSaving}>
			<HugeiconsIcon icon={RefreshIcon} class="size-4" strokeWidth={2} />
			{storageSaving ? 'Testing…' : 'Save & test connection'}
		</Button>
	</Card.Footer>
</Card.Root>

{#if backupError}
	<Alert.Root variant="destructive" class="mb-4">
		<HugeiconsIcon icon={Alert02Icon} class="size-4" strokeWidth={2} />
		<Alert.Description>{backupError}</Alert.Description>
	</Alert.Root>
{/if}

<Card.Root class="mb-6 border-border/60">
	<Card.Header class="flex-row items-start justify-between gap-4">
		<div>
			<Card.Title class="flex items-center gap-2">
				<HugeiconsIcon icon={CloudBackupIcon} class="size-5 text-primary" strokeWidth={2} />
				Backup policy
			</Card.Title>
			<Card.Description>Global defaults for new backup schedules and retention behavior</Card.Description>
		</div>
		{#if backupMessage}
			<Badge variant="default">{backupMessage}</Badge>
		{/if}
	</Card.Header>
	<Card.Content class="space-y-6">
		<div class="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
			<div class="space-y-2">
				<Label>Retention (days)</Label>
				<Input type="number" min="1" max="3650" bind:value={backupPolicy.retention_days} />
			</div>
			<div class="space-y-2">
				<Label>Keep at most</Label>
				<Input type="number" min="1" max="10000" bind:value={backupPolicy.keep_count} />
			</div>
			<div class="space-y-2">
				<Label>Default cron</Label>
				<Input bind:value={backupPolicy.cron_default} class="font-mono" placeholder="0 3 * * *" />
			</div>
			<div class="space-y-2">
				<Label>Schedule hour (UTC)</Label>
				<Input type="number" min="0" max="23" bind:value={backupPolicy.schedule_hour} />
			</div>
			<div class="space-y-2">
				<Label>Compression (0–9)</Label>
				<Input type="number" min="0" max="9" bind:value={backupPolicy.compression_level} />
			</div>
			<div class="space-y-2">
				<Label>Parallel jobs</Label>
				<Input type="number" min="1" max="16" bind:value={backupPolicy.parallel_jobs} />
			</div>
			<div class="space-y-2">
				<Label>Dump format</Label>
				<Select.Root type="single" bind:value={backupPolicy.dump_format}>
					<Select.Trigger class="w-full">{backupPolicy.dump_format}</Select.Trigger>
					<Select.Content>
						<Select.Item value="custom" label="custom">custom</Select.Item>
						<Select.Item value="plain" label="plain">plain</Select.Item>
						<Select.Item value="directory" label="directory">directory</Select.Item>
					</Select.Content>
				</Select.Root>
			</div>
			<div class="space-y-2 md:col-span-2">
				<Label>Notify webhook URL</Label>
				<Input bind:value={backupPolicy.notify_webhook} placeholder="https://hooks.example.com/..." />
			</div>
			<div class="space-y-2 md:col-span-2 lg:col-span-3">
				<Label>Exclude schemas (default)</Label>
				<Input bind:value={backupPolicy.exclude_schemas_default} class="font-mono" placeholder="pg_catalog,information_schema" />
			</div>
		</div>
		<div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
			<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
				<span>Verify after backup</span>
				<Switch bind:checked={backupPolicy.verify_after} />
			</label>
			<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
				<span>Keep local copy</span>
				<Switch bind:checked={backupPolicy.keep_local_copy} />
			</label>
			<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
				<span>Notify on success</span>
				<Switch bind:checked={backupPolicy.notify_on_success} />
			</label>
			<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
				<span>Notify on failure</span>
				<Switch bind:checked={backupPolicy.notify_on_failure} />
			</label>
			<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
				<span>WAL archiving default</span>
				<Switch bind:checked={backupPolicy.wal_archiving_default} />
			</label>
			<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
				<span>Encrypt backups</span>
				<Switch bind:checked={backupPolicy.encrypt} />
			</label>
		</div>
	</Card.Content>
	<Card.Footer class="justify-end">
		<Button onclick={saveBackupPolicy} disabled={backupSaving}>
			<HugeiconsIcon icon={RefreshIcon} class="size-4" strokeWidth={2} />
			{backupSaving ? 'Saving…' : 'Save backup policy'}
		</Button>
	</Card.Footer>
</Card.Root>

<Card.Root class="mb-6 border-border/60">
	<Card.Header class="flex-row items-start justify-between gap-4">
		<div>
			<Card.Title class="flex items-center gap-2">
				<HugeiconsIcon icon={Key01Icon} class="size-5 text-primary" strokeWidth={2} />
				API tokens
			</Card.Title>
			<Card.Description>Programmatic access with scoped roles — shown once at creation</Card.Description>
		</div>
	</Card.Header>
	<Card.Content class="space-y-6">
		{#if createdToken}
			<Alert.Root class="border-primary/40 bg-primary/5">
				<HugeiconsIcon icon={Key01Icon} class="size-4" strokeWidth={2} />
				<Alert.Title>Copy your new token</Alert.Title>
				<Alert.Description class="space-y-3">
					<p class="text-sm">This token will not be shown again.</p>
					<code class="block break-all rounded-lg bg-muted/60 p-3 font-mono text-xs">{createdToken.token}</code>
					<Button size="sm" variant="outline" onclick={copyToken}>
						<HugeiconsIcon icon={Copy01Icon} class="size-4" strokeWidth={2} />
						Copy token
					</Button>
				</Alert.Description>
			</Alert.Root>
		{/if}

		<form class="grid gap-4 md:grid-cols-2 lg:grid-cols-4" onsubmit={createToken}>
			<div class="space-y-2 lg:col-span-2">
				<Label for="token-name">Name</Label>
				<Input id="token-name" bind:value={tokenName} placeholder="CI deploy" required />
			</div>
			<div class="space-y-2">
				<Label>Role</Label>
				<Select.Root type="single" bind:value={tokenRole}>
					<Select.Trigger class="w-full capitalize">{tokenRole}</Select.Trigger>
					<Select.Content>
						<Select.Item value="admin" label="admin">admin</Select.Item>
						<Select.Item value="operator" label="operator">operator</Select.Item>
						<Select.Item value="viewer" label="viewer">viewer</Select.Item>
					</Select.Content>
				</Select.Root>
			</div>
			<div class="space-y-2">
				<Label for="expires-days">Expires (days)</Label>
				<Input
					id="expires-days"
					type="number"
					min="1"
					bind:value={tokenExpiresDays}
					placeholder="Never"
				/>
			</div>
			<div class="space-y-2 md:col-span-2 lg:col-span-4">
				<Label for="token-scopes">Scopes</Label>
				<Input id="token-scopes" bind:value={tokenScopes} class="font-mono" placeholder="read,write" />
			</div>
			<div class="md:col-span-2 lg:col-span-4">
				<Button type="submit" disabled={tokenCreating}>
					<HugeiconsIcon icon={Key01Icon} class="size-4" strokeWidth={2} />
					{tokenCreating ? 'Creating…' : 'Create token'}
				</Button>
			</div>
		</form>

		{#if tokens.length > 0}
			<div class="rounded-lg border border-border/60">
				<table class="w-full text-sm">
					<thead>
						<tr class="border-b border-border/60 text-left text-xs text-muted-foreground">
							<th class="p-3 font-medium">Name</th>
							<th class="p-3 font-medium">Prefix</th>
							<th class="p-3 font-medium">Role</th>
							<th class="p-3 font-medium">Status</th>
							<th class="p-3 text-right font-medium">Actions</th>
						</tr>
					</thead>
					<tbody>
						{#each tokens as t (t.id)}
							<tr class="border-b border-border/40 last:border-0">
								<td class="p-3 font-medium">{t.name}</td>
								<td class="p-3 font-mono text-xs">{t.token_prefix}…</td>
								<td class="p-3 capitalize">{t.role}</td>
								<td class="p-3">
									{#if t.revoked_at}
										<Badge variant="destructive">revoked</Badge>
									{:else if t.expires_at && new Date(t.expires_at) < new Date()}
										<Badge variant="secondary">expired</Badge>
									{:else}
										<Badge variant="default">active</Badge>
									{/if}
								</td>
								<td class="p-3 text-right">
									{#if !t.revoked_at}
										<Button variant="ghost" size="sm" onclick={() => askRevokeToken(t.id)}>
											<HugeiconsIcon icon={Delete02Icon} class="size-4" strokeWidth={2} />
											Revoke
										</Button>
									{/if}
								</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
		{:else}
			<p class="text-sm text-muted-foreground">No API tokens yet.</p>
		{/if}
	</Card.Content>
</Card.Root>

<div class="grid gap-4 md:grid-cols-2">
	<Card.Root class="border-border/60">
		<Card.Header>
			<Card.Title>Panel health</Card.Title>
			<Card.Description>Local process status</Card.Description>
		</Card.Header>
		<Card.Content class="space-y-3 text-sm">
			<div class="flex items-center justify-between">
				<span class="text-muted-foreground">API status</span>
				{#if health}
					<Badge variant={health.status === 'ok' ? 'default' : 'destructive'}>{health.status}</Badge>
				{:else}
					<span>—</span>
				{/if}
			</div>
			<Separator />
			<div class="flex items-center justify-between">
				<span class="text-muted-foreground">Version</span>
				<span class="font-mono text-xs">{health?.version ?? '—'}</span>
			</div>
			<Separator />
			<div class="flex items-center justify-between">
				<span class="text-muted-foreground">SQLite</span>
				{#if ready}
					<span class="inline-flex items-center gap-1.5">
						{#if ready.database}
							<HugeiconsIcon icon={CheckmarkCircle02Icon} class="size-4 text-primary" strokeWidth={2} />
							ok
						{:else}
							<HugeiconsIcon icon={Alert02Icon} class="size-4 text-destructive" strokeWidth={2} />
							down
						{/if}
					</span>
				{:else}
					—
				{/if}
			</div>
			<Separator />
			<div class="flex items-center justify-between">
				<span class="text-muted-foreground">Docker</span>
				{#if ready}
					<span class="inline-flex items-center gap-1.5">
						{#if ready.docker}
							<HugeiconsIcon icon={CheckmarkCircle02Icon} class="size-4 text-primary" strokeWidth={2} />
							ok
						{:else}
							<HugeiconsIcon icon={Alert02Icon} class="size-4 text-destructive" strokeWidth={2} />
							down
						{/if}
					</span>
				{:else}
					—
				{/if}
			</div>
		</Card.Content>
	</Card.Root>

	<Card.Root class="border-border/60">
		<Card.Header>
			<Card.Title>Security notes</Card.Title>
			<Card.Description>Hardening reminders for operators</Card.Description>
		</Card.Header>
		<Card.Content>
			<ul class="space-y-3 text-sm text-muted-foreground">
				<li class="flex gap-2">
					<span class="mt-1 size-1.5 shrink-0 rounded-full bg-primary"></span>
					Admin password is Argon2id-hashed; sessions are server-side cookies.
				</li>
				<li class="flex gap-2">
					<span class="mt-1 size-1.5 shrink-0 rounded-full bg-primary"></span>
					PostgreSQL credentials are AES-256-GCM encrypted at rest.
				</li>
				<li class="flex gap-2">
					<span class="mt-1 size-1.5 shrink-0 rounded-full bg-primary"></span>
					Cookies: HttpOnly, SameSite, Secure in production.
				</li>
				<li class="flex gap-2">
					<span class="mt-1 size-1.5 shrink-0 rounded-full bg-amber-400"></span>
					Docker socket on the panel is a privileged risk — see SECURITY.md.
				</li>
			</ul>
		</Card.Content>
	</Card.Root>
</div>

<ConfirmDialog
	bind:open={revokeOpen}
	title="Revoke API token?"
	description="This token will stop working immediately."
	confirmLabel="Revoke"
	variant="destructive"
	onConfirm={revokeToken}
/>

<ConfirmDialog
	bind:open={applyOpen}
	title="Apply panel update?"
	description="The panel will download and install the latest version. It may restart and briefly become unavailable."
	confirmLabel="Update now"
	loading={applying}
	onConfirm={handleApplyUpdate}
/>

{#if updateOverlay}
	<div
		class="fixed inset-0 z-50 flex items-center justify-center bg-background/80 backdrop-blur-sm"
		role="alertdialog"
		aria-live="polite"
		aria-busy="true"
		aria-label="Panel update in progress"
	>
		<div class="mx-4 max-w-md rounded-xl border border-border/60 bg-popover p-8 text-center shadow-lg">
			<HugeiconsIcon icon={RefreshIcon} class="mx-auto mb-4 size-10 animate-spin text-primary" strokeWidth={2} />
			<h2 class="mb-2 text-lg font-semibold">Updating…</h2>
			<p class="text-sm text-muted-foreground">
				Near-zero cutover: image pull first (no downtime), then a short panel recreate. This tab reconnects automatically.
			</p>
		</div>
	</div>
{/if}
