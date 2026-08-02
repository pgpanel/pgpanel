<script lang="ts">
	import { goto } from '$app/navigation';
	import { api } from '$lib/api';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Input } from '$lib/components/ui/input/index.js';
	import { Label } from '$lib/components/ui/label/index.js';
	import { Switch } from '$lib/components/ui/switch/index.js';
	import * as Card from '$lib/components/ui/card/index.js';
	import * as Select from '$lib/components/ui/select/index.js';
	import { toast } from 'svelte-sonner';
	import { trackOperation } from '$lib/jobs';
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import {
		DatabaseIcon,
		Shield01Icon,
		CloudUploadIcon,
		CheckmarkCircle02Icon
	} from '@hugeicons/core-free-icons';

	let step = $state(0);
	let saving = $state(false);

	// Step: welcome
	// Step: first cluster
	let name = $state('primary');
	let postgres_version = $state('17');
	let enable_backup = $state(true);
	let db_name = $state('app');
	let role_name = $state('app_user');
	let cpu_limit = $state(2);
	let memory_mb = $state(2048);
	let storage_limit_gb = $state(20);
	// Step: backup storage
	let storage_type = $state('local');
	let storage_endpoint = $state('');
	let storage_region = $state('auto');
	let storage_bucket = $state('');
	let storage_prefix = $state('pgpanel/');
	let storage_access_key = $state('');
	let storage_secret_key = $state('');
	let storage_path_style = $state(true);
	let storage_tls_verify = $state(true);

	const steps = ['Welcome', 'First cluster', 'Backups', 'Done'];

	async function exitWizard() {
		try {
			await api('/api/settings/wizard', {
				method: 'POST',
				body: JSON.stringify({ completed: true })
			});
		} finally {
			sessionStorage.setItem('pgpanel_wizard_done', '1');
			goto('/dashboard');
		}
	}

	async function completeWizard() {
		try {
			if (storage_type !== 'local') {
				if (!storage_bucket || !storage_access_key || !storage_secret_key) {
					toast.error('Bucket, access key and secret key are required for object storage');
					step = 2;
					return;
				}
				await api('/api/settings/storage', {
					method: 'POST',
					body: JSON.stringify({
						storage_type,
						endpoint: storage_endpoint,
						region: storage_region,
						bucket: storage_bucket,
						prefix: storage_prefix,
						access_key: storage_access_key,
						secret_key: storage_secret_key,
						path_style: storage_path_style,
						tls_verify: storage_tls_verify,
						encrypt: true,
						retention_days: 14,
						keep_count: 30,
						schedule_hour: 3
					})
				});
			}
			await api('/api/settings/wizard', {
				method: 'POST',
				body: JSON.stringify({ completed: true })
			});
			sessionStorage.setItem('pgpanel_wizard_done', '1');
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Could not save wizard settings');
			return;
		}
		sessionStorage.setItem('pgpanel_wizard_done', '1');
		goto('/dashboard');
	}

	async function createFirstCluster() {
		saving = true;
		try {
			const res = await api<{
				cluster: { id: string };
				admin_password: string;
				operation_id: string;
			}>('/api/clusters', {
				method: 'POST',
				body: JSON.stringify({
					name,
					postgres_version,
					cpu_limit,
					memory_mb,
					storage_limit_gb,
					expose_publicly: false,
					enable_backup,
					initial_databases:
						db_name && role_name
							? [{ database_name: db_name, role_name }]
							: []
				})
			});
			toast.message('Admin password (copy now)', {
				description: res.admin_password,
				duration: 20000
			});
			trackOperation(res.operation_id, {
				title: `Create ${name}`,
				onDone: () => {
					step = 2;
				}
			});
			step = 2;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Create failed');
		} finally {
			saving = false;
		}
	}
</script>

<div class="mx-auto flex min-h-[80vh] max-w-2xl flex-col justify-center gap-6 py-8">
	<div class="text-center">
		<p class="text-sm font-medium tracking-wide text-sky-400 uppercase">PgPanel setup</p>
		<h1 class="mt-1 text-3xl font-semibold tracking-tight">Get started</h1>
		<p class="mt-2 text-sm text-muted-foreground">
			A short wizard — domain and admin are already set. Configure your first database.
		</p>
	</div>

	<div class="flex justify-center gap-2">
		{#each steps as s, i}
			<div
				class="flex items-center gap-2 rounded-full px-3 py-1 text-xs {i === step
					? 'bg-primary text-primary-foreground'
					: i < step
						? 'bg-emerald-500/20 text-emerald-300'
						: 'bg-muted text-muted-foreground'}"
			>
				{i + 1}. {s}
			</div>
		{/each}
	</div>
	<div class="flex justify-center">
		<Button variant="ghost" size="sm" onclick={exitWizard}>Exit setup</Button>
	</div>

	{#if step === 0}
		<Card.Root class="border-border/60">
			<Card.Header>
				<Card.Title class="flex items-center gap-2">
					<HugeiconsIcon icon={Shield01Icon} class="size-5" strokeWidth={2} />
					Welcome
				</Card.Title>
				<Card.Description>
					PgPanel manages isolated PostgreSQL clusters on this host. Backups are built-in (no
					Databasus). Clusters stay on private Docker networks by default.
				</Card.Description>
			</Card.Header>
			<Card.Content class="space-y-3 text-sm text-muted-foreground">
				<p>• Create clusters, databases, and application roles</p>
				<p>• Native logical backups (pg_dump) + optional object storage</p>
				<p>• Live operation progress in the UI</p>
				<p>• Resource monitoring per cluster</p>
			</Card.Content>
			<Card.Footer class="justify-end">
				<Button onclick={() => (step = 1)}>Continue</Button>
			</Card.Footer>
		</Card.Root>
	{:else if step === 1}
		<Card.Root class="border-border/60">
			<Card.Header>
				<Card.Title class="flex items-center gap-2">
					<HugeiconsIcon icon={DatabaseIcon} class="size-5" strokeWidth={2} />
					First cluster
				</Card.Title>
				<Card.Description>
					Creates a PostgreSQL instance and optional app database + role in one step.
				</Card.Description>
			</Card.Header>
			<Card.Content class="space-y-4">
				<div class="space-y-2">
					<Label>Cluster name</Label>
					<Input bind:value={name} required />
				</div>
				<div class="space-y-2">
					<Label>PostgreSQL version</Label>
					<Select.Root type="single" bind:value={postgres_version}>
						<Select.Trigger class="w-full">PostgreSQL {postgres_version}</Select.Trigger>
						<Select.Content>
							<Select.Item value="16" label="16">16</Select.Item>
							<Select.Item value="17" label="17">17</Select.Item>
							<Select.Item value="18" label="18">18</Select.Item>
						</Select.Content>
					</Select.Root>
				</div>
				<div class="grid gap-4 sm:grid-cols-2">
					<div class="space-y-2">
						<Label>App database</Label>
						<Input bind:value={db_name} class="font-mono" placeholder="app" />
					</div>
					<div class="space-y-2">
						<Label>App role</Label>
						<Input bind:value={role_name} class="font-mono" placeholder="app_user" />
					</div>
				</div>
				<div class="grid gap-4 sm:grid-cols-3">
					<div class="space-y-2">
						<Label>CPU limit</Label>
						<Input type="number" min="1" max="32" bind:value={cpu_limit} />
					</div>
					<div class="space-y-2">
						<Label>Memory (MB)</Label>
						<Input type="number" min="512" max="262144" bind:value={memory_mb} />
					</div>
					<div class="space-y-2">
						<Label>Storage (GB)</Label>
						<Input type="number" min="1" max="100000" bind:value={storage_limit_gb} />
					</div>
				</div>
				<div class="flex items-center justify-between">
					<div>
						<Label>Enable backups</Label>
						<p class="text-xs text-muted-foreground">Native pg_dump schedule (daily 03:00 UTC)</p>
					</div>
					<Switch bind:checked={enable_backup} />
				</div>
			</Card.Content>
			<Card.Footer class="justify-between">
				<Button variant="outline" onclick={() => (step = 0)}>Back</Button>
				<div class="flex gap-2">
					<Button variant="ghost" onclick={() => (step = 2)}>Skip</Button>
					<Button disabled={saving} onclick={createFirstCluster}>
						{saving ? 'Creating…' : 'Create cluster'}
					</Button>
				</div>
			</Card.Footer>
		</Card.Root>
	{:else if step === 2}
		<Card.Root class="border-border/60">
			<Card.Header>
				<Card.Title class="flex items-center gap-2">
					<HugeiconsIcon icon={CloudUploadIcon} class="size-5" strokeWidth={2} />
					Backup storage
				</Card.Title>
				<Card.Description>
					Choose where backups live. Credentials are encrypted in the panel database and never written
					to <code class="text-xs">.env</code>.
				</Card.Description>
			</Card.Header>
			<Card.Content class="space-y-3 text-sm">
				<label class="flex cursor-pointer items-center gap-3 rounded-lg border p-3">
					<input type="radio" bind:group={storage_type} value="local" />
					<span>
						<strong>Local</strong>
						<span class="block text-xs text-muted-foreground">Fast · on this VPS</span>
					</span>
				</label>
				<label class="flex cursor-pointer items-center gap-3 rounded-lg border p-3">
					<input type="radio" bind:group={storage_type} value="s3" />
					<span>
						<strong>S3-compatible</strong>
						<span class="block text-xs text-muted-foreground"
							>AWS, R2, B2, Hetzner or MinIO</span
						>
					</span>
				</label>
				{#if storage_type !== 'local'}
					<div class="grid gap-4 border-t pt-4 sm:grid-cols-2">
						<div class="space-y-2 sm:col-span-2">
							<Label>Endpoint URL</Label>
							<Input bind:value={storage_endpoint} placeholder="https://..." />
						</div>
						<div class="space-y-2">
							<Label>Region</Label>
							<Input bind:value={storage_region} placeholder="auto" />
						</div>
						<div class="space-y-2">
							<Label>Bucket</Label>
							<Input bind:value={storage_bucket} required />
						</div>
						<div class="space-y-2">
							<Label>Access key</Label>
							<Input bind:value={storage_access_key} autocomplete="off" required />
						</div>
						<div class="space-y-2">
							<Label>Secret key</Label>
							<Input type="password" bind:value={storage_secret_key} autocomplete="new-password" required />
						</div>
					</div>
				{/if}
			</Card.Content>
			<Card.Footer class="justify-between">
				<Button variant="outline" onclick={() => (step = 1)}>Back</Button>
				<Button onclick={() => (step = 3)}>Continue</Button>
			</Card.Footer>
		</Card.Root>
	{:else}
		<Card.Root class="border-border/60">
			<Card.Header>
				<Card.Title class="flex items-center gap-2 text-emerald-400">
					<HugeiconsIcon icon={CheckmarkCircle02Icon} class="size-5" strokeWidth={2} />
					You're ready
				</Card.Title>
				<Card.Description>
					Open the dashboard to manage clusters, run backups, and monitor resources.
				</Card.Description>
			</Card.Header>
			<Card.Footer class="justify-end">
				<Button onclick={completeWizard}>Go to dashboard</Button>
			</Card.Footer>
		</Card.Root>
	{/if}
</div>
