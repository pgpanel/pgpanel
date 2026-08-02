<script lang="ts">
	import { onMount } from 'svelte';
	import { api } from '$lib/api';
	import PageHeader from '$lib/components/page-header.svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Input } from '$lib/components/ui/input/index.js';
	import { Label } from '$lib/components/ui/label/index.js';
	import { Switch } from '$lib/components/ui/switch/index.js';
	import * as Select from '$lib/components/ui/select/index.js';
	import * as Card from '$lib/components/ui/card/index.js';
	import * as Alert from '$lib/components/ui/alert/index.js';
	import { Badge } from '$lib/components/ui/badge/index.js';
	import { Separator } from '$lib/components/ui/separator/index.js';
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import { CheckmarkCircle02Icon, Alert02Icon, CloudUploadIcon, RefreshIcon } from '@hugeicons/core-free-icons';

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

	onMount(async () => {
		try {
			[health, ready, storage] = await Promise.all([
				api<{ status: string; version: string }>('/health'),
				api<{ ready: boolean; database: boolean; docker: boolean }>('/ready'),
				api<typeof storage>('/api/settings/storage')
			]);
		} catch {
			storageError = 'Could not load the current storage configuration';
		}
	});

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

<PageHeader title="Settings" description="Storage, backup policy, health and security posture" />

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
