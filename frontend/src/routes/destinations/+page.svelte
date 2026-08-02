<script lang="ts">
	import { onMount } from 'svelte';
	import { api, type BackupDestination, formatRelative } from '$lib/api';
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
	import * as Select from '$lib/components/ui/select/index.js';
	import { Badge } from '$lib/components/ui/badge/index.js';
	import { toast } from 'svelte-sonner';
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import {
		Alert02Icon,
		CloudUploadIcon,
		PlusSignIcon,
		RefreshIcon,
		Delete02Icon,
		InformationCircleIcon,
		SecurityCheckIcon,
		Edit02Icon
	} from '@hugeicons/core-free-icons';

	let destinations = $state<BackupDestination[]>([]);
	let error = $state('');
	let loading = $state(true);
	let saving = $state(false);
	let testing = $state<string | null>(null);
	let editingId = $state<string | null>(null);

	let name = $state('');
	let storage_type = $state('s3');
	let endpoint = $state('');
	let region = $state('auto');
	let bucket = $state('');
	let prefix = $state('pgpanel/');
	let path_style = $state(true);
	let tls_verify = $state(true);
	let encrypt_backups = $state(true);
	let compression_level = $state(6);
	let access_key = $state('');
	let secret_key = $state('');
	let notes = $state('');
	let set_default = $state(false);
	let enabled = $state(true);

	async function load() {
		destinations = await api<BackupDestination[]>('/api/backup-destinations');
	}

	onMount(() => {
		load()
			.catch((e) => (error = e instanceof Error ? e.message : 'Failed to load destinations'))
			.finally(() => (loading = false));
	});

	function resetForm() {
		editingId = null;
		name = '';
		storage_type = 's3';
		endpoint = '';
		region = 'auto';
		bucket = '';
		prefix = 'pgpanel/';
		path_style = true;
		tls_verify = true;
		encrypt_backups = true;
		compression_level = 6;
		access_key = '';
		secret_key = '';
		notes = '';
		set_default = false;
		enabled = true;
	}

	function startEdit(d: BackupDestination) {
		editingId = d.id;
		name = d.name;
		storage_type = d.storage_type;
		endpoint = d.endpoint;
		region = d.region;
		bucket = d.bucket;
		prefix = d.prefix;
		path_style = d.path_style;
		tls_verify = d.tls_verify;
		encrypt_backups = d.encrypt_backups;
		compression_level = d.compression_level;
		access_key = '';
		secret_key = '';
		notes = d.notes ?? '';
		set_default = d.is_default;
		enabled = d.enabled;
	}

	async function save(e: Event) {
		e.preventDefault();
		saving = true;
		error = '';
		try {
			const body = {
				name,
				storage_type,
				endpoint,
				region,
				bucket,
				prefix,
				path_style,
				tls_verify,
				encrypt_backups,
				compression_level,
				access_key,
				secret_key,
				notes: notes || null,
				set_default,
				enabled
			};
			if (editingId) {
				await api<BackupDestination>(`/api/backup-destinations/${editingId}`, {
					method: 'PUT',
					body: JSON.stringify(body)
				});
				toast.success('Destination updated');
			} else {
				await api<BackupDestination>('/api/backup-destinations', {
					method: 'POST',
					body: JSON.stringify(body)
				});
				toast.success('Destination created');
			}
			resetForm();
			await load();
		} catch (e) {
			error = e instanceof Error ? e.message : 'Save failed';
			toast.error(error);
		} finally {
			saving = false;
		}
	}

	async function testDestination(id: string) {
		testing = id;
		try {
			await api(`/api/backup-destinations/${id}/test`, { method: 'POST' });
			toast.success('Connection test passed');
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Test failed');
			await load();
		} finally {
			testing = null;
		}
	}

	async function deleteDestination(d: BackupDestination) {
		if (!confirm(`Delete destination "${d.name}"?`)) return;
		try {
			await api(`/api/backup-destinations/${d.id}`, { method: 'DELETE' });
			toast.success('Destination deleted');
			if (editingId === d.id) resetForm();
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Delete failed');
		}
	}
</script>

<PageHeader
	title="Backup destinations"
	description="Shared object storage targets — any cluster or node can bind to any destination"
/>

<Alert.Root class="mb-4 border-border/60">
	<HugeiconsIcon icon={InformationCircleIcon} class="size-4" strokeWidth={2} />
	<Alert.Description>
		Destinations are global. Bind them to clusters from each cluster's backup page under
		<strong>Backup targets</strong>.
	</Alert.Description>
</Alert.Root>

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
				<HugeiconsIcon icon={CloudUploadIcon} class="size-5 text-primary" strokeWidth={2} />
				{editingId ? 'Edit destination' : 'Add destination'}
			</Card.Title>
			<Card.Description>S3-compatible storage or local disk</Card.Description>
		</Card.Header>
		<Card.Content>
			<form class="space-y-4" onsubmit={save}>
				<div class="space-y-2">
					<Label for="dest-name">Name</Label>
					<Input id="dest-name" bind:value={name} required />
				</div>
				<div class="space-y-2">
					<Label>Storage type</Label>
					<Select.Root type="single" bind:value={storage_type}>
						<Select.Trigger class="w-full uppercase">{storage_type}</Select.Trigger>
						<Select.Content>
							<Select.Item value="local" label="local">Local disk</Select.Item>
							<Select.Item value="s3" label="s3">S3 / compatible</Select.Item>
							<Select.Item value="r2" label="r2">Cloudflare R2</Select.Item>
							<Select.Item value="b2" label="b2">Backblaze B2</Select.Item>
							<Select.Item value="hetzner" label="hetzner">Hetzner</Select.Item>
							<Select.Item value="minio" label="minio">MinIO</Select.Item>
						</Select.Content>
					</Select.Root>
				</div>
				{#if storage_type !== 'local'}
					<div class="space-y-2">
						<Label>Endpoint</Label>
						<Input bind:value={endpoint} placeholder="https://s3.amazonaws.com" />
					</div>
					<div class="grid gap-4 sm:grid-cols-2">
						<div class="space-y-2">
							<Label>Region</Label>
							<Input bind:value={region} />
						</div>
						<div class="space-y-2">
							<Label>Bucket</Label>
							<Input bind:value={bucket} required />
						</div>
					</div>
					<div class="space-y-2">
						<Label>Prefix</Label>
						<Input bind:value={prefix} class="font-mono" />
					</div>
					<div class="grid gap-4 sm:grid-cols-2">
						<div class="space-y-2">
							<Label>Access key</Label>
							<Input bind:value={access_key} autocomplete="off" />
						</div>
						<div class="space-y-2">
							<Label>Secret key</Label>
							<Input type="password" bind:value={secret_key} autocomplete="new-password" />
						</div>
					</div>
				{/if}
				<div class="space-y-2">
					<Label>Compression (0–9)</Label>
					<Input type="number" min="0" max="9" bind:value={compression_level} />
				</div>
				<div class="space-y-2">
					<Label>Notes</Label>
					<Textarea bind:value={notes} rows={2} />
				</div>
				<div class="grid gap-3 sm:grid-cols-2">
					<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
						<span>Encrypt backups</span>
						<Switch bind:checked={encrypt_backups} />
					</label>
					<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
						<span>Set as default</span>
						<Switch bind:checked={set_default} />
					</label>
					<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
						<span>Enabled</span>
						<Switch bind:checked={enabled} />
					</label>
					{#if storage_type !== 'local'}
						<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
							<span>Path-style</span>
							<Switch bind:checked={path_style} />
						</label>
						<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
							<span>TLS verify</span>
							<Switch bind:checked={tls_verify} />
						</label>
					{/if}
				</div>
				<div class="flex gap-2">
					<Button type="submit" disabled={saving} class="flex-1">
						<HugeiconsIcon icon={PlusSignIcon} class="size-4" strokeWidth={2} />
						{saving ? 'Saving…' : editingId ? 'Update' : 'Create'}
					</Button>
					{#if editingId}
						<Button type="button" variant="outline" onclick={resetForm}>Cancel</Button>
					{/if}
				</div>
			</form>
		</Card.Content>
	</Card.Root>

	<Card.Root class="border-border/60">
		<Card.Header class="flex-row items-center justify-between space-y-0">
			<div>
				<Card.Title>Destinations</Card.Title>
				<Card.Description>Reusable backup storage endpoints</Card.Description>
			</div>
			<Button variant="outline" size="sm" onclick={() => load()}>
				<HugeiconsIcon icon={RefreshIcon} class="size-4" strokeWidth={2} />
				Refresh
			</Button>
		</Card.Header>
		<Card.Content class="p-0">
			{#if !loading && destinations.length === 0}
				<div class="p-6">
					<EmptyState title="No destinations" description="Add a backup destination to get started." />
				</div>
			{:else}
				<Table.Root>
					<Table.Header>
						<Table.Row class="hover:bg-transparent">
							<Table.Head>Name</Table.Head>
							<Table.Head>Type</Table.Head>
							<Table.Head>Test</Table.Head>
							<Table.Head class="text-right">Actions</Table.Head>
						</Table.Row>
					</Table.Header>
					<Table.Body>
						{#each destinations as d (d.id)}
							<Table.Row>
								<Table.Cell>
									<div class="flex items-center gap-2">
										<span class="font-medium">{d.name}</span>
										{#if d.is_default}
											<Badge variant="secondary" class="text-[10px]">default</Badge>
										{/if}
									</div>
									<div class="font-mono text-xs text-muted-foreground">{d.slug}</div>
									{#if d.bucket}
										<div class="text-xs text-muted-foreground">{d.bucket}</div>
									{/if}
								</Table.Cell>
								<Table.Cell>
									<Badge variant="outline" class="uppercase">{d.storage_type}</Badge>
									{#if !d.enabled}
										<StatusBadge status="disabled" />
									{/if}
								</Table.Cell>
								<Table.Cell class="text-xs">
									{#if d.last_test_at}
										{#if d.last_test_ok}
											<span class="text-emerald-400">OK</span>
										{:else}
											<span class="text-destructive" title={d.last_test_error ?? ''}>Failed</span>
										{/if}
										<div class="text-muted-foreground">{formatRelative(d.last_test_at)}</div>
									{:else}
										—
									{/if}
								</Table.Cell>
								<Table.Cell class="text-right">
									<div class="flex justify-end gap-1">
										<Button
											variant="ghost"
											size="sm"
											disabled={testing === d.id}
											onclick={() => testDestination(d.id)}
										>
											<HugeiconsIcon icon={SecurityCheckIcon} class="size-4" strokeWidth={2} />
											{testing === d.id ? '…' : 'Test'}
										</Button>
										<Button variant="ghost" size="sm" onclick={() => startEdit(d)}>
											<HugeiconsIcon icon={Edit02Icon} class="size-4" strokeWidth={2} />
										</Button>
										<Button
											variant="ghost"
											size="sm"
											class="text-destructive hover:text-destructive"
											onclick={() => deleteDestination(d)}
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
