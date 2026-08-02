<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/stores';
	import { api, type Cluster } from '$lib/api';
	import PageHeader from '$lib/components/page-header.svelte';
	import StatusBadge from '$lib/components/status-badge.svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import * as Card from '$lib/components/ui/card/index.js';
	import * as Alert from '$lib/components/ui/alert/index.js';
	import { Separator } from '$lib/components/ui/separator/index.js';
	import { toast } from 'svelte-sonner';
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import {
		PlayIcon,
		StopIcon,
		RefreshIcon,
		DatabaseIcon,
		TableIcon,
		CodeIcon,
		CloudBackupIcon,
		DatabaseSyncIcon,
		Alert02Icon
	} from '@hugeicons/core-free-icons';

	let cluster = $state<Cluster | null>(null);
	let error = $state('');
	let busy = $state('');

	const id = $derived($page.params.id);

	async function load() {
		cluster = await api<Cluster>(`/api/clusters/${id}`);
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

	const links = $derived([
		{ href: `/clusters/${id}/databases`, label: 'Databases', icon: DatabaseIcon },
		{ href: `/clusters/${id}/browser`, label: 'Browser', icon: TableIcon },
		{ href: `/clusters/${id}/query`, label: 'SQL console', icon: CodeIcon },
		{ href: `/clusters/${id}/backup`, label: 'Backups', icon: CloudBackupIcon },
		{ href: `/clusters/${id}/backup?tab=wal`, label: 'WAL / PITR', icon: DatabaseSyncIcon },
		{ href: `/replicas?cluster=${id}`, label: 'Replicas', icon: DatabaseSyncIcon }
	]);
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
		{/snippet}
	</PageHeader>

	{#if error}
		<Alert.Root variant="destructive" class="mb-4">
			<HugeiconsIcon icon={Alert02Icon} class="size-4" strokeWidth={2} />
			<Alert.Description>{error}</Alert.Description>
		</Alert.Root>
	{/if}

	<div class="mb-6 flex flex-wrap gap-2">
		{#each links as l}
			<Button variant="secondary" href={l.href}>
				<HugeiconsIcon icon={l.icon} class="size-4" strokeWidth={2} />
				{l.label}
			</Button>
		{/each}
	</div>

	<div class="grid gap-4 md:grid-cols-2">
		<Card.Root class="border-border/60">
			<Card.Header>
				<Card.Title>Connection & resources</Card.Title>
			</Card.Header>
			<Card.Content class="space-y-3 text-sm">
				<div class="flex justify-between gap-4">
					<span class="text-muted-foreground">Internal host</span>
					<span class="font-mono text-right">{cluster.internal_hostname ?? '—'}</span>
				</div>
				<Separator />
				<div class="flex justify-between gap-4">
					<span class="text-muted-foreground">Public port</span>
					<span>{cluster.public_port ?? 'not exposed'}</span>
				</div>
				<Separator />
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
					<span class="text-muted-foreground">Databasus</span>
					<span class="font-mono text-xs">{cluster.databasus_status}</span>
				</div>
				<Separator />
				<div class="flex justify-between gap-4">
					<span class="text-muted-foreground">Delete protection</span>
					<span>{cluster.delete_protection ? 'on' : 'off'}</span>
				</div>
			</Card.Content>
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
