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
	// Step: backup storage note
	let storage_type = $state('local');

	const steps = ['Welcome', 'First cluster', 'Backups', 'Done'];

	async function completeWizard() {
		try {
			await api('/api/settings/wizard', {
				method: 'POST',
				body: JSON.stringify({ completed: true })
			});
			sessionStorage.setItem('pgpanel_wizard_done', '1');
		} catch {
			sessionStorage.setItem('pgpanel_wizard_done', '1');
		}
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
					cpu_limit: 1,
					memory_mb: 1024,
					storage_limit_gb: 20,
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
					Default is local disk under the panel data directory. Configure S3/R2 in Settings or
					<code class="text-xs">.env</code> (BACKUP_STORAGE_TYPE, S3_*).
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
				<label class="flex cursor-pointer items-center gap-3 rounded-lg border p-3 opacity-90">
					<input type="radio" bind:group={storage_type} value="s3" />
					<span>
						<strong>S3-compatible</strong>
						<span class="block text-xs text-muted-foreground"
							>R2 / B2 / MinIO / AWS — set keys in .env then restart panel</span
						>
					</span>
				</label>
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
