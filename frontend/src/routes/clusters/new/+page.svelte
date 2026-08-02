<script lang="ts">
	import { goto } from '$app/navigation';
	import { api } from '$lib/api';
	import PageHeader from '$lib/components/page-header.svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Input } from '$lib/components/ui/input/index.js';
	import { Label } from '$lib/components/ui/label/index.js';
	import { Switch } from '$lib/components/ui/switch/index.js';
	import * as Card from '$lib/components/ui/card/index.js';
	import * as Alert from '$lib/components/ui/alert/index.js';
	import * as Select from '$lib/components/ui/select/index.js';
	import { toast } from 'svelte-sonner';
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import { Alert02Icon, InformationCircleIcon, Copy01Icon } from '@hugeicons/core-free-icons';

	let name = $state('');
	let postgres_version = $state('17');
	let cpu_limit = $state(1);
	let memory_mb = $state(1024);
	let storage_limit_gb = $state(20);
	let expose_publicly = $state(false);
	let optional_public_port = $state(5433);
	let enable_backup = $state(true);
	let error = $state('');
	let oneTimePassword = $state('');
	let loading = $state(false);
	let createdId = $state('');

	const versionLabel = $derived(`PostgreSQL ${postgres_version}`);

	async function submit(e: Event) {
		e.preventDefault();
		loading = true;
		error = '';
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
					expose_publicly,
					optional_public_port: expose_publicly ? optional_public_port : null,
					enable_backup
				})
			});
			oneTimePassword = res.admin_password;
			createdId = res.cluster.id;
			toast.success('Cluster creation queued');
		} catch (err) {
			error = err instanceof Error ? err.message : 'Create failed';
			toast.error(error);
		} finally {
			loading = false;
		}
	}

	async function copyPw() {
		await navigator.clipboard.writeText(oneTimePassword);
		toast.success('Password copied');
	}
</script>

<PageHeader
	title="Create cluster"
	description="Provision an isolated PostgreSQL instance with private Docker networking"
/>

{#if oneTimePassword}
	<Alert.Root class="mb-6 border-amber-500/40 bg-amber-500/10 text-amber-50">
		<HugeiconsIcon icon={InformationCircleIcon} class="size-4" strokeWidth={2} />
		<Alert.Title>One-time admin password</Alert.Title>
		<Alert.Description class="mt-2 space-y-3">
			<p>Copy now — it will not be shown again.</p>
			<div class="flex flex-wrap items-center gap-2">
				<code class="rounded-md bg-black/40 px-3 py-2 font-mono text-sm break-all">{oneTimePassword}</code>
				<Button size="sm" variant="secondary" onclick={copyPw}>
					<HugeiconsIcon icon={Copy01Icon} class="size-4" strokeWidth={2} />
					Copy
				</Button>
			</div>
			<Button size="sm" href={`/clusters/${createdId}`}>Go to cluster</Button>
		</Alert.Description>
	</Alert.Root>
{/if}

<form class="max-w-2xl space-y-6" onsubmit={submit}>
	{#if error}
		<Alert.Root variant="destructive">
			<HugeiconsIcon icon={Alert02Icon} class="size-4" strokeWidth={2} />
			<Alert.Description>{error}</Alert.Description>
		</Alert.Root>
	{/if}

	<Card.Root class="border-border/60">
		<Card.Header>
			<Card.Title>Basics</Card.Title>
			<Card.Description>Name and PostgreSQL version (allowlisted images only)</Card.Description>
		</Card.Header>
		<Card.Content class="space-y-4">
			<div class="space-y-2">
				<Label for="name">Display name</Label>
				<Input id="name" bind:value={name} placeholder="Production Primary" required />
			</div>
			<div class="space-y-2">
				<Label>PostgreSQL version</Label>
				<Select.Root type="single" bind:value={postgres_version}>
					<Select.Trigger class="w-full">{versionLabel}</Select.Trigger>
					<Select.Content>
						<Select.Item value="16" label="PostgreSQL 16">PostgreSQL 16</Select.Item>
						<Select.Item value="17" label="PostgreSQL 17">PostgreSQL 17</Select.Item>
						<Select.Item value="18" label="PostgreSQL 18">PostgreSQL 18</Select.Item>
					</Select.Content>
				</Select.Root>
			</div>
		</Card.Content>
	</Card.Root>

	<Card.Root class="border-border/60">
		<Card.Header>
			<Card.Title>Resources</Card.Title>
			<Card.Description>CPU, memory and soft storage limit</Card.Description>
		</Card.Header>
		<Card.Content>
			<div class="grid gap-4 sm:grid-cols-3">
				<div class="space-y-2">
					<Label for="cpu">CPU</Label>
					<Input id="cpu" type="number" step="0.1" min="0.1" bind:value={cpu_limit} />
				</div>
				<div class="space-y-2">
					<Label for="mem">Memory (MB)</Label>
					<Input id="mem" type="number" min="128" bind:value={memory_mb} />
				</div>
				<div class="space-y-2">
					<Label for="disk">Storage (GB)</Label>
					<Input id="disk" type="number" min="1" bind:value={storage_limit_gb} />
				</div>
			</div>
		</Card.Content>
	</Card.Root>

	<Card.Root class="border-border/60">
		<Card.Header>
			<Card.Title>Networking & backup</Card.Title>
		</Card.Header>
		<Card.Content class="space-y-5">
			<div class="flex items-center justify-between gap-4">
				<div class="space-y-0.5">
					<Label>Databasus backup integration</Label>
					<p class="text-xs text-muted-foreground">Register cluster for backups when healthy</p>
				</div>
				<Switch bind:checked={enable_backup} />
			</div>
			<div class="flex items-center justify-between gap-4">
				<div class="space-y-0.5">
					<Label>Expose public port</Label>
					<p class="text-xs text-muted-foreground">Disabled by default — prefer private networks</p>
				</div>
				<Switch bind:checked={expose_publicly} />
			</div>
			{#if expose_publicly}
				<div class="space-y-2">
					<Label for="port">Public port (≥ 1024)</Label>
					<Input id="port" type="number" min="1024" bind:value={optional_public_port} />
				</div>
			{/if}
		</Card.Content>
		<Card.Footer class="justify-end gap-2 border-t">
			<Button variant="outline" href="/clusters" type="button">Cancel</Button>
			<Button type="submit" disabled={loading}>{loading ? 'Creating…' : 'Create cluster'}</Button>
		</Card.Footer>
	</Card.Root>
</form>
