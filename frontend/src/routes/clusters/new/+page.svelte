<script lang="ts">
	import { onMount } from 'svelte';
	import { api, type Node } from '$lib/api';
	import { trackOperation } from '$lib/jobs';
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
	let db_name = $state('');
	let role_name = $state('');
	let role_password = $state('');
	let error = $state('');
	let oneTimePassword = $state('');
	let appPassword = $state('');
	let loading = $state(false);
	let createdId = $state('');
	let opProgress = $state('');
	let nodes = $state<Node[]>([]);
	let node_id = $state('');

	const versionLabel = $derived(`PostgreSQL ${postgres_version}`);
	const namePattern = '^[a-z][a-z0-9_]{2,62}$';
	const selectedNodeLabel = $derived(
		nodes.find((n) => n.id === node_id)?.name ?? 'Select node'
	);

	onMount(async () => {
		try {
			nodes = await api<Node[]>('/api/nodes');
			const defaultNode = nodes.find((n) => n.is_default) ?? nodes[0];
			if (defaultNode) node_id = defaultNode.id;
		} catch {
			/* optional — cluster can still be created on local node */
		}
	});

	async function submit(e: Event) {
		e.preventDefault();
		loading = true;
		error = '';
		try {
			const initial_databases =
				db_name && role_name
					? [
							{
								database_name: db_name,
								role_name,
								password: role_password || undefined
							}
						]
					: [];

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
					enable_backup,
					node_id: node_id || undefined,
					initial_databases
				})
			});
			oneTimePassword = res.admin_password;
			createdId = res.cluster.id;
			opProgress = 'queued';
			trackOperation(res.operation_id, {
				title: `Create cluster “${name}”`,
				onDone: (op) => {
					opProgress = 'succeeded';
					const result = op.result as {
						initial_credentials?: { database: string; role: string; password: string }[];
					} | null;
					const cred = result?.initial_credentials?.[0];
					if (cred?.password) {
						appPassword = cred.password;
						toast.message('App role password (copy now)', {
							description: `${cred.role}@${cred.database}`,
							duration: 15000
						});
					}
				},
				onFail: () => {
					opProgress = 'failed';
				}
			});
		} catch (err) {
			error = err instanceof Error ? err.message : 'Create failed';
			toast.error(error);
		} finally {
			loading = false;
		}
	}

	async function copyPw(text: string) {
		await navigator.clipboard.writeText(text);
		toast.success('Copied');
	}
</script>

<PageHeader
	title="Create cluster"
	description="Provision isolated PostgreSQL · private Docker network · native backups"
/>

{#if oneTimePassword}
	<Alert.Root class="mb-6 border-amber-500/40 bg-amber-500/10 text-amber-50">
		<HugeiconsIcon icon={InformationCircleIcon} class="size-4" strokeWidth={2} />
		<Alert.Title>Credentials (copy now)</Alert.Title>
		<Alert.Description class="mt-2 space-y-3">
			<p class="text-xs opacity-80">
				Provisioning status: <strong>{opProgress || 'running'}</strong> — toast shows live progress.
			</p>
			<div>
				<p class="mb-1 text-xs font-medium">postgres superuser</p>
				<div class="flex flex-wrap items-center gap-2">
					<code class="rounded-md bg-black/40 px-3 py-2 font-mono text-sm break-all"
						>{oneTimePassword}</code
					>
					<Button size="sm" variant="secondary" onclick={() => copyPw(oneTimePassword)}>
						<HugeiconsIcon icon={Copy01Icon} class="size-4" strokeWidth={2} />
						Copy
					</Button>
				</div>
			</div>
			{#if appPassword}
				<div>
					<p class="mb-1 text-xs font-medium">App role ({role_name})</p>
					<div class="flex flex-wrap items-center gap-2">
						<code class="rounded-md bg-black/40 px-3 py-2 font-mono text-sm break-all"
							>{appPassword}</code
						>
						<Button size="sm" variant="secondary" onclick={() => copyPw(appPassword)}>
							Copy
						</Button>
					</div>
				</div>
			{/if}
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
			{#if nodes.length > 0}
				<div class="space-y-2">
					<Label>Docker node</Label>
					<Select.Root type="single" bind:value={node_id}>
						<Select.Trigger class="w-full">{selectedNodeLabel}</Select.Trigger>
						<Select.Content>
							{#each nodes as node}
								<Select.Item value={node.id} label={node.name}>
									{node.name}
									{#if node.is_default}
										<span class="text-muted-foreground"> (default)</span>
									{/if}
									{#if node.kind === 'local'}
										<span class="text-muted-foreground"> · local</span>
									{/if}
								</Select.Item>
							{/each}
						</Select.Content>
					</Select.Root>
					<p class="text-xs text-muted-foreground">
						Clusters are provisioned on the selected Docker host.
					</p>
				</div>
			{/if}
		</Card.Content>
	</Card.Root>

	<Card.Root class="border-border/60">
		<Card.Header>
			<Card.Title>Application database & role</Card.Title>
			<Card.Description>
				Created automatically after the cluster is healthy. Leave empty to skip.
			</Card.Description>
		</Card.Header>
		<Card.Content class="grid gap-4 sm:grid-cols-2">
			<div class="space-y-2">
				<Label for="db">Database name</Label>
				<Input
					id="db"
					bind:value={db_name}
					pattern={namePattern}
					class="font-mono"
					placeholder="app"
				/>
			</div>
			<div class="space-y-2">
				<Label for="role">Role name</Label>
				<Input
					id="role"
					bind:value={role_name}
					pattern={namePattern}
					class="font-mono"
					placeholder="app_user"
				/>
			</div>
			<div class="space-y-2 sm:col-span-2">
				<Label for="rpw">Role password (optional — generated if empty)</Label>
				<Input id="rpw" type="password" bind:value={role_password} autocomplete="new-password" />
			</div>
		</Card.Content>
	</Card.Root>

	<Card.Root class="border-border/60">
		<Card.Header>
			<Card.Title>Resources</Card.Title>
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
					<Label>Native backups</Label>
					<p class="text-xs text-muted-foreground">
						pg_dump schedule + retention (no external Databasus)
					</p>
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
