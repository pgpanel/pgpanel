<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/stores';
	import { api } from '$lib/api';
	import PageHeader from '$lib/components/page-header.svelte';
	import StatusBadge from '$lib/components/status-badge.svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import * as Card from '$lib/components/ui/card/index.js';
	import * as Alert from '$lib/components/ui/alert/index.js';
	import { Separator } from '$lib/components/ui/separator/index.js';
	import { toast } from 'svelte-sonner';
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import {
		ArrowLeft01Icon,
		Alert02Icon,
		RefreshIcon,
		CloudBackupIcon,
		InformationCircleIcon
	} from '@hugeicons/core-free-icons';

	interface BackupStatus {
		integration_status: string;
		last_successful_backup: string | null;
		last_backup_status: string | null;
		backup_lag_seconds: number | null;
		wal_status: string | null;
		failed_backups: number;
		message: string | null;
		manual_setup_info: unknown;
	}

	const id = $derived($page.params.id);
	let status = $state<BackupStatus | null>(null);
	let error = $state('');

	async function load() {
		status = await api<BackupStatus>(`/api/clusters/${id}/backup`);
	}

	onMount(() => {
		load().catch((e) => (error = e.message));
	});

	async function register() {
		error = '';
		try {
			const res = await api<{ operation_id: string }>(`/api/clusters/${id}/backup/register`, {
				method: 'POST'
			});
			toast.success('Registration queued', { description: res.operation_id });
			await load();
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed';
			toast.error(error);
		}
	}

	async function trigger() {
		error = '';
		try {
			await api(`/api/clusters/${id}/backup/trigger`, { method: 'POST' });
			toast.success('Backup triggered');
			await load();
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed';
			toast.error(error);
		}
	}
</script>

<div class="mb-2">
	<Button variant="ghost" size="sm" href={`/clusters/${id}`}>
		<HugeiconsIcon icon={ArrowLeft01Icon} class="size-4" strokeWidth={2} />
		Back to cluster
	</Button>
</div>

<PageHeader
	title="Backup (Databasus)"
	description="WAL, PITR and restore verification are handled by Databasus — not reimplemented here"
>
	{#snippet actions()}
		<Button variant="outline" onclick={register}>
			<HugeiconsIcon icon={RefreshIcon} class="size-4" strokeWidth={2} />
			Register / retry
		</Button>
		<Button onclick={trigger}>
			<HugeiconsIcon icon={CloudBackupIcon} class="size-4" strokeWidth={2} />
			Trigger backup
		</Button>
	{/snippet}
</PageHeader>

{#if error}
	<Alert.Root variant="destructive" class="mb-4">
		<HugeiconsIcon icon={Alert02Icon} class="size-4" strokeWidth={2} />
		<Alert.Description>{error}</Alert.Description>
	</Alert.Root>
{/if}

{#if status}
	<div class="grid gap-4 lg:grid-cols-2">
		<Card.Root class="border-border/60">
			<Card.Header class="flex-row items-center justify-between space-y-0">
				<Card.Title>Integration</Card.Title>
				<StatusBadge status={status.integration_status} />
			</Card.Header>
			<Card.Content class="space-y-3 text-sm">
				<div class="flex justify-between gap-4">
					<span class="text-muted-foreground">Last success</span>
					<span class="text-right text-xs">{status.last_successful_backup ?? '—'}</span>
				</div>
				<Separator />
				<div class="flex justify-between gap-4">
					<span class="text-muted-foreground">Last status</span>
					<span>{status.last_backup_status ?? '—'}</span>
				</div>
				<Separator />
				<div class="flex justify-between gap-4">
					<span class="text-muted-foreground">Lag (s)</span>
					<span>{status.backup_lag_seconds ?? '—'}</span>
				</div>
				<Separator />
				<div class="flex justify-between gap-4">
					<span class="text-muted-foreground">WAL</span>
					<span>{status.wal_status ?? '—'}</span>
				</div>
				<Separator />
				<div class="flex justify-between gap-4">
					<span class="text-muted-foreground">Failed backups</span>
					<span class="tabular-nums">{status.failed_backups}</span>
				</div>
				{#if status.message}
					<p class="pt-2 text-muted-foreground">{status.message}</p>
				{/if}
			</Card.Content>
		</Card.Root>

		{#if status.manual_setup_info}
			<Card.Root class="border-border/60">
				<Card.Header>
					<Card.Title class="flex items-center gap-2">
						<HugeiconsIcon icon={InformationCircleIcon} class="size-4" strokeWidth={2} />
						Manual setup
					</Card.Title>
					<Card.Description>
						Databasus API paths are version-dependent — complete in UI if needed
					</Card.Description>
				</Card.Header>
				<Card.Content>
					<pre class="overflow-x-auto rounded-lg bg-muted/40 p-3 font-mono text-xs">{JSON.stringify(
							status.manual_setup_info,
							null,
							2
						)}</pre>
				</Card.Content>
			</Card.Root>
		{/if}
	</div>
{/if}
