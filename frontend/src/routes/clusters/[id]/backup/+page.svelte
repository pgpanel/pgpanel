<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/stores';
	import { api, type BackupSchedule, type Cluster, type DatabaseRecord, type BackupDestination, type ClusterBackupTarget } from '$lib/api';
	import { trackOperation } from '$lib/jobs';
	import PageHeader from '$lib/components/page-header.svelte';
	import StatusBadge from '$lib/components/status-badge.svelte';
	import ConfirmDialog from '$lib/components/confirm-dialog.svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Input } from '$lib/components/ui/input/index.js';
	import { Label } from '$lib/components/ui/label/index.js';
	import { Switch } from '$lib/components/ui/switch/index.js';
	import { Textarea } from '$lib/components/ui/textarea/index.js';
	import * as Card from '$lib/components/ui/card/index.js';
	import * as Alert from '$lib/components/ui/alert/index.js';
	import * as Table from '$lib/components/ui/table/index.js';
	import * as Select from '$lib/components/ui/select/index.js';
	import { Separator } from '$lib/components/ui/separator/index.js';
	import { toast } from 'svelte-sonner';
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import {
		ArrowLeft01Icon,
		Alert02Icon,
		RefreshIcon,
		CloudBackupIcon,
		InformationCircleIcon,
		SecurityCheckIcon,
		Delete02Icon,
		RestoreBinIcon,
		Calendar03Icon,
		Target01Icon
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

	interface BackupRow {
		id: string;
		kind: string;
		status: string;
		database_name: string;
		storage_key: string | null;
		size_bytes: number | null;
		checksum_sha256: string | null;
		error: string | null;
		created_at: string;
	}

	const id = $derived($page.params.id);
	let cluster = $state<Cluster | null>(null);
	let status = $state<BackupStatus | null>(null);
	let history = $state<BackupRow[]>([]);
	let schedules = $state<BackupSchedule[]>([]);
	let databases = $state<DatabaseRecord[]>([]);
	let destinations = $state<BackupDestination[]>([]);
	let targets = $state<ClusterBackupTarget[]>([]);
	let error = $state('');

	let triggerDatabase = $state('postgres');
	let triggerSchemaOnly = $state(false);

	let scheduleCron = $state('0 3 * * *');
	let scheduleDatabase = $state('postgres');
	let scheduleRetention = $state(14);
	let scheduleKeepCount = $state(30);
	let scheduleCompression = $state(6);
	let scheduleSchemaOnly = $state(false);
	let scheduleExcludeSchemas = $state('');
	let scheduleExcludeTables = $state('');
	let scheduleVerifyAfter = $state(false);
	let scheduleNotifySuccess = $state(false);
	let scheduleNotifyFailure = $state(true);
	let scheduleWindowStart = $state<number | ''>('');
	let scheduleWindowEnd = $state<number | ''>('');
	let scheduleDescription = $state('');
	let scheduleEnabled = $state(true);
	let scheduleSaving = $state(false);

	let deleteScheduleOpen = $state(false);
	let deleteScheduleId = $state<string | null>(null);
	let deleteTargetOpen = $state(false);
	let deleteTargetId = $state<string | null>(null);

	let targetDestinationId = $state('');
	let targetCron = $state('0 3 * * *');
	let targetRetention = $state(14);
	let targetKeepCount = $state(30);
	let targetExcludeSchemas = $state('');
	let targetExcludeTables = $state('');
	let targetVerifyAfter = $state(false);
	let targetSchemaOnly = $state(false);
	let targetEnabled = $state(true);
	let targetSaving = $state(false);

	let restoreBackupId = $state('');
	let restoreTargetDb = $state('');
	let restoreClean = $state(false);
	let restoreConfirmName = $state('');
	let restoring = $state(false);

	const succeededBackups = $derived(history.filter((b) => b.status === 'succeeded' || b.status === 'success'));

	async function load() {
		const [st, h, sch, dbs, cl, dests, tgts] = await Promise.all([
			api<BackupStatus>(`/api/clusters/${id}/backup`),
			api<{ backups: BackupRow[] }>(`/api/clusters/${id}/backup/history`),
			api<BackupSchedule[]>(`/api/clusters/${id}/backup/schedules`),
			api<DatabaseRecord[]>(`/api/clusters/${id}/databases`).catch(() => [] as DatabaseRecord[]),
			api<Cluster>(`/api/clusters/${id}`),
			api<BackupDestination[]>('/api/backup-destinations').catch(() => [] as BackupDestination[]),
			api<ClusterBackupTarget[]>(`/api/clusters/${id}/backup-targets`).catch(() => [] as ClusterBackupTarget[])
		]);
		status = st;
		history = h.backups ?? [];
		schedules = sch;
		databases = dbs;
		cluster = cl;
		destinations = dests;
		targets = tgts;
		if (!targetDestinationId && dests.length > 0) {
			targetDestinationId = dests.find((d) => d.is_default)?.id ?? dests[0].id;
		}
		if (dbs.length > 0 && !dbs.some((d) => d.name === triggerDatabase)) {
			triggerDatabase = dbs[0].name;
		}
		if (!restoreBackupId && succeededBackups.length > 0) {
			restoreBackupId = succeededBackups[0].id;
		}
	}

	onMount(() => {
		load().catch((e) => (error = e.message));
	});

	async function enable() {
		error = '';
		try {
			const res = await api<{ operation_id: string }>(`/api/clusters/${id}/backup/enable`, {
				method: 'POST'
			});
			trackOperation(res.operation_id, {
				title: 'Enable backups',
				onDone: () => load()
			});
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed';
			toast.error(error);
		}
	}

	async function trigger() {
		error = '';
		try {
			const res = await api<{ operation_id: string }>(`/api/clusters/${id}/backup/trigger`, {
				method: 'POST',
				body: JSON.stringify({
					database: triggerDatabase,
					schema_only: triggerSchemaOnly
				})
			});
			trackOperation(res.operation_id, {
				title: 'Logical backup',
				onDone: () => load(),
				onFail: () => load()
			});
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed';
			toast.error(error);
		}
	}

	async function verify() {
		error = '';
		try {
			const res = await api<{ operation_id: string }>(`/api/clusters/${id}/backup/verify`, {
				method: 'POST'
			});
			trackOperation(res.operation_id, {
				title: 'Verify backup',
				onDone: () => load()
			});
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed';
			toast.error(error);
		}
	}

	async function prune() {
		error = '';
		try {
			const res = await api<{ operation_id: string }>(`/api/clusters/${id}/backup/prune`, {
				method: 'POST'
			});
			trackOperation(res.operation_id, {
				title: 'Prune old backups',
				onDone: () => load()
			});
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed';
			toast.error(error);
		}
	}

	async function saveSchedule(e: Event) {
		e.preventDefault();
		scheduleSaving = true;
		try {
			await api<BackupSchedule>(`/api/clusters/${id}/backup/schedules`, {
				method: 'POST',
				body: JSON.stringify({
					cron: scheduleCron,
					database_name: scheduleDatabase,
					enabled: scheduleEnabled,
					retention_days: scheduleRetention,
					keep_count: scheduleKeepCount,
					compression_level: scheduleCompression,
					schema_only: scheduleSchemaOnly,
					exclude_schemas: scheduleExcludeSchemas,
					exclude_tables: scheduleExcludeTables,
					verify_after: scheduleVerifyAfter,
					notify_on_success: scheduleNotifySuccess,
					notify_on_failure: scheduleNotifyFailure,
					window_start_hour: scheduleWindowStart === '' ? null : scheduleWindowStart,
					window_end_hour: scheduleWindowEnd === '' ? null : scheduleWindowEnd,
					description: scheduleDescription || null
				})
			});
			toast.success('Schedule saved');
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to save schedule');
		} finally {
			scheduleSaving = false;
		}
	}

	function askDeleteSchedule(sid: string) {
		deleteScheduleId = sid;
		deleteScheduleOpen = true;
	}

	async function confirmDeleteSchedule() {
		if (!deleteScheduleId) return;
		try {
			await api(`/api/clusters/${id}/backup/schedules/${deleteScheduleId}`, { method: 'DELETE' });
			toast.success('Schedule deleted');
			deleteScheduleId = null;
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Delete failed');
		}
	}

	async function saveTarget(e: Event) {
		e.preventDefault();
		if (!targetDestinationId) {
			toast.error('Select a destination');
			return;
		}
		targetSaving = true;
		try {
			await api<ClusterBackupTarget>(`/api/clusters/${id}/backup-targets`, {
				method: 'POST',
				body: JSON.stringify({
					destination_id: targetDestinationId,
					enabled: targetEnabled,
					cron: targetCron,
					retention_days: targetRetention,
					keep_count: targetKeepCount,
					exclude_schemas: targetExcludeSchemas,
					exclude_tables: targetExcludeTables,
					verify_after: targetVerifyAfter,
					schema_only: targetSchemaOnly
				})
			});
			toast.success('Backup target saved');
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to save target');
		} finally {
			targetSaving = false;
		}
	}

	function askDeleteTarget(tid: string) {
		deleteTargetId = tid;
		deleteTargetOpen = true;
	}

	async function confirmDeleteTarget() {
		if (!deleteTargetId) return;
		try {
			await api(`/api/clusters/${id}/backup-targets/${deleteTargetId}`, { method: 'DELETE' });
			toast.success('Target removed');
			deleteTargetId = null;
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Delete failed');
		}
	}

	async function restore() {
		if (!restoreBackupId || !restoreTargetDb.trim()) {
			toast.error('Select a backup and target database');
			return;
		}
		if (cluster && restoreConfirmName !== cluster.name) {
			toast.error('Cluster name confirmation does not match');
			return;
		}
		restoring = true;
		try {
			const res = await api<{ operation_id: string }>(`/api/clusters/${id}/backup/restore`, {
				method: 'POST',
				body: JSON.stringify({
					backup_id: restoreBackupId,
					target_database: restoreTargetDb,
					clean: restoreClean,
					confirm_cluster_name: restoreConfirmName
				})
			});
			trackOperation(res.operation_id, {
				title: 'Restore backup',
				onDone: () => load()
			});
			restoreConfirmName = '';
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Restore failed');
		} finally {
			restoring = false;
		}
	}

	function fmtSize(n: number | null) {
		if (n == null) return '—';
		if (n < 1024) return `${n} B`;
		if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
		return `${(n / 1024 / 1024).toFixed(2)} MB`;
	}
</script>

<div class="mb-2">
	<Button variant="ghost" size="sm" href={`/clusters/${id}`}>
		<HugeiconsIcon icon={ArrowLeft01Icon} class="size-4" strokeWidth={2} />
		Back to cluster
	</Button>
</div>

<PageHeader
	title="Backups"
	description="Native engine · pg_dump / pg_restore · local or S3 · scheduled retention"
>
	{#snippet actions()}
		<Button variant="outline" onclick={enable}>
			<HugeiconsIcon icon={RefreshIcon} class="size-4" strokeWidth={2} />
			Enable / schedule
		</Button>
		<Button variant="outline" onclick={verify}>
			<HugeiconsIcon icon={SecurityCheckIcon} class="size-4" strokeWidth={2} />
			Verify latest
		</Button>
		<Button variant="outline" onclick={prune}>
			<HugeiconsIcon icon={Delete02Icon} class="size-4" strokeWidth={2} />
			Prune
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
	<div class="mb-6 grid gap-4 lg:grid-cols-2">
		<Card.Root class="border-border/60">
			<Card.Header class="flex-row items-center justify-between space-y-0">
				<Card.Title>Status</Card.Title>
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
					<span class="text-muted-foreground">Failed</span>
					<span class="tabular-nums">{status.failed_backups}</span>
				</div>
				<Separator />
				<div class="flex justify-between gap-4">
					<span class="text-muted-foreground">WAL / engine</span>
					<span class="text-right text-xs">{status.wal_status ?? 'native'}</span>
				</div>
				{#if status.message}
					<p class="pt-2 text-muted-foreground">{status.message}</p>
				{/if}
			</Card.Content>
		</Card.Root>

		<Card.Root class="border-border/60">
			<Card.Header>
				<Card.Title class="flex items-center gap-2">
					<HugeiconsIcon icon={CloudBackupIcon} class="size-4" strokeWidth={2} />
					Run backup now
				</Card.Title>
				<Card.Description>Trigger an on-demand logical dump</Card.Description>
			</Card.Header>
			<Card.Content class="space-y-4">
				<div class="space-y-2">
					<Label>Database</Label>
					{#if databases.length > 0}
						<Select.Root type="single" bind:value={triggerDatabase}>
							<Select.Trigger class="w-full font-mono">{triggerDatabase}</Select.Trigger>
							<Select.Content>
								{#each databases as db (db.id)}
									<Select.Item value={db.name} label={db.name}>{db.name}</Select.Item>
								{/each}
							</Select.Content>
						</Select.Root>
					{:else}
						<Input bind:value={triggerDatabase} class="font-mono" placeholder="postgres" />
					{/if}
				</div>
				<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
					<span>Schema only</span>
					<Switch bind:checked={triggerSchemaOnly} />
				</label>
				<Button onclick={trigger} class="w-full">
					<HugeiconsIcon icon={CloudBackupIcon} class="size-4" strokeWidth={2} />
					Run backup
				</Button>
			</Card.Content>
		</Card.Root>
	</div>

	<Card.Root class="mb-6 border-border/60">
		<Card.Header>
			<Card.Title class="flex items-center gap-2">
				<HugeiconsIcon icon={InformationCircleIcon} class="size-4" strokeWidth={2} />
				Engine config
			</Card.Title>
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

<Card.Root class="mb-6 border-border/60">
	<Card.Header>
		<Card.Title class="flex items-center gap-2">
			<HugeiconsIcon icon={Calendar03Icon} class="size-4" strokeWidth={2} />
			Schedules
		</Card.Title>
		<Card.Description>Per-database cron schedules with retention and notification options</Card.Description>
	</Card.Header>
	<Card.Content class="space-y-6">
		{#if schedules.length > 0}
			<Table.Root>
				<Table.Header>
					<Table.Row class="hover:bg-transparent">
						<Table.Head>Database</Table.Head>
						<Table.Head>Cron</Table.Head>
						<Table.Head>Retention</Table.Head>
						<Table.Head>Enabled</Table.Head>
						<Table.Head>Next run</Table.Head>
						<Table.Head></Table.Head>
					</Table.Row>
				</Table.Header>
				<Table.Body>
					{#each schedules as s (s.id)}
						<Table.Row>
							<Table.Cell class="font-mono text-xs">{s.database_name}</Table.Cell>
							<Table.Cell class="font-mono text-xs">{s.cron}</Table.Cell>
							<Table.Cell class="text-xs">{s.retention_days}d / {s.keep_count}</Table.Cell>
							<Table.Cell>
								<StatusBadge status={s.enabled ? 'enabled' : 'disabled'} />
							</Table.Cell>
							<Table.Cell class="text-xs">{s.next_run_at ?? '—'}</Table.Cell>
							<Table.Cell>
								<Button variant="ghost" size="sm" onclick={() => askDeleteSchedule(s.id)}>
									<HugeiconsIcon icon={Delete02Icon} class="size-4" strokeWidth={2} />
								</Button>
							</Table.Cell>
						</Table.Row>
					{/each}
				</Table.Body>
			</Table.Root>
		{:else}
			<p class="text-sm text-muted-foreground">No schedules yet — create one below.</p>
		{/if}

		<form class="space-y-4 border-t border-border/60 pt-5" onsubmit={saveSchedule}>
			<p class="text-sm font-medium">Upsert schedule</p>
			<div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
				<div class="space-y-2">
					<Label>Cron expression</Label>
					<Input bind:value={scheduleCron} class="font-mono" placeholder="0 3 * * *" required />
				</div>
				<div class="space-y-2">
					<Label>Database</Label>
					<Input bind:value={scheduleDatabase} class="font-mono" required />
				</div>
				<div class="space-y-2">
					<Label>Description</Label>
					<Input bind:value={scheduleDescription} placeholder="Nightly full backup" />
				</div>
				<div class="space-y-2">
					<Label>Retention (days)</Label>
					<Input type="number" min="1" bind:value={scheduleRetention} />
				</div>
				<div class="space-y-2">
					<Label>Keep count</Label>
					<Input type="number" min="1" bind:value={scheduleKeepCount} />
				</div>
				<div class="space-y-2">
					<Label>Compression (0–9)</Label>
					<Input type="number" min="0" max="9" bind:value={scheduleCompression} />
				</div>
				<div class="space-y-2">
					<Label>Window start hour</Label>
					<Input type="number" min="0" max="23" bind:value={scheduleWindowStart} placeholder="—" />
				</div>
				<div class="space-y-2">
					<Label>Window end hour</Label>
					<Input type="number" min="0" max="23" bind:value={scheduleWindowEnd} placeholder="—" />
				</div>
			</div>
			<div class="grid gap-4 sm:grid-cols-2">
				<div class="space-y-2">
					<Label>Exclude schemas</Label>
					<Textarea bind:value={scheduleExcludeSchemas} rows={2} class="font-mono text-xs" />
				</div>
				<div class="space-y-2">
					<Label>Exclude tables</Label>
					<Textarea bind:value={scheduleExcludeTables} rows={2} class="font-mono text-xs" />
				</div>
			</div>
			<div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
				<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
					<span>Enabled</span>
					<Switch bind:checked={scheduleEnabled} />
				</label>
				<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
					<span>Schema only</span>
					<Switch bind:checked={scheduleSchemaOnly} />
				</label>
				<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
					<span>Verify after</span>
					<Switch bind:checked={scheduleVerifyAfter} />
				</label>
				<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
					<span>Notify on success</span>
					<Switch bind:checked={scheduleNotifySuccess} />
				</label>
				<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
					<span>Notify on failure</span>
					<Switch bind:checked={scheduleNotifyFailure} />
				</label>
			</div>
			<Button type="submit" disabled={scheduleSaving}>
				{scheduleSaving ? 'Saving…' : 'Save schedule'}
			</Button>
		</form>
	</Card.Content>
</Card.Root>

<Card.Root class="mb-6 border-border/60">
	<Card.Header>
		<Card.Title class="flex items-center gap-2">
			<HugeiconsIcon icon={Target01Icon} class="size-4" strokeWidth={2} />
			Backup targets
		</Card.Title>
		<Card.Description>
			Bind shared destinations to this cluster — any node can target any destination
		</Card.Description>
	</Card.Header>
	<Card.Content class="space-y-6">
		{#if targets.length > 0}
			<Table.Root>
				<Table.Header>
					<Table.Row class="hover:bg-transparent">
						<Table.Head>Destination</Table.Head>
						<Table.Head>Cron</Table.Head>
						<Table.Head>Retention</Table.Head>
						<Table.Head>Status</Table.Head>
						<Table.Head></Table.Head>
					</Table.Row>
				</Table.Header>
				<Table.Body>
					{#each targets as t (t.id)}
						<Table.Row>
							<Table.Cell class="font-medium">{t.destination_name}</Table.Cell>
							<Table.Cell class="font-mono text-xs">{t.cron}</Table.Cell>
							<Table.Cell class="text-xs">{t.retention_days}d / {t.keep_count}</Table.Cell>
							<Table.Cell>
								<StatusBadge status={t.enabled ? (t.last_status ?? 'enabled') : 'disabled'} />
							</Table.Cell>
							<Table.Cell>
								<Button variant="ghost" size="sm" onclick={() => askDeleteTarget(t.id)}>
									<HugeiconsIcon icon={Delete02Icon} class="size-4" strokeWidth={2} />
								</Button>
							</Table.Cell>
						</Table.Row>
					{/each}
				</Table.Body>
			</Table.Root>
		{:else}
			<p class="text-sm text-muted-foreground">No backup targets — bind a destination below.</p>
		{/if}

		<form class="space-y-4 border-t border-border/60 pt-5" onsubmit={saveTarget}>
			<p class="text-sm font-medium">Bind destination</p>
			{#if destinations.length === 0}
				<p class="text-sm text-muted-foreground">
					No destinations configured.
					<a href="/destinations" class="text-primary hover:underline">Add one first</a>.
				</p>
			{:else}
				<div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
					<div class="space-y-2">
						<Label>Destination</Label>
						<Select.Root type="single" bind:value={targetDestinationId}>
							<Select.Trigger class="w-full">
								{destinations.find((d) => d.id === targetDestinationId)?.name ?? 'Select'}
							</Select.Trigger>
							<Select.Content>
								{#each destinations as d (d.id)}
									<Select.Item value={d.id} label={d.name}>{d.name}</Select.Item>
								{/each}
							</Select.Content>
						</Select.Root>
					</div>
					<div class="space-y-2">
						<Label>Cron</Label>
						<Input bind:value={targetCron} class="font-mono" required />
					</div>
					<div class="space-y-2">
						<Label>Retention (days)</Label>
						<Input type="number" min="1" bind:value={targetRetention} />
					</div>
					<div class="space-y-2">
						<Label>Keep count</Label>
						<Input type="number" min="1" bind:value={targetKeepCount} />
					</div>
				</div>
				<div class="grid gap-4 sm:grid-cols-2">
					<div class="space-y-2">
						<Label>Exclude schemas</Label>
						<Textarea bind:value={targetExcludeSchemas} rows={2} class="font-mono text-xs" />
					</div>
					<div class="space-y-2">
						<Label>Exclude tables</Label>
						<Textarea bind:value={targetExcludeTables} rows={2} class="font-mono text-xs" />
					</div>
				</div>
				<div class="grid gap-3 sm:grid-cols-3">
					<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
						<span>Enabled</span>
						<Switch bind:checked={targetEnabled} />
					</label>
					<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
						<span>Verify after</span>
						<Switch bind:checked={targetVerifyAfter} />
					</label>
					<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
						<span>Schema only</span>
						<Switch bind:checked={targetSchemaOnly} />
					</label>
				</div>
				<Button type="submit" disabled={targetSaving}>
					{targetSaving ? 'Saving…' : 'Save target'}
				</Button>
			{/if}
		</form>
	</Card.Content>
</Card.Root>

<Card.Root class="mb-6 border-border/60">
	<Card.Header>
		<Card.Title class="flex items-center gap-2">
			<HugeiconsIcon icon={RestoreBinIcon} class="size-4" strokeWidth={2} />
			Restore
		</Card.Title>
		<Card.Description>Restore a backup into a target database — destructive if clean is enabled</Card.Description>
	</Card.Header>
	<Card.Content class="space-y-4">
		<div class="grid gap-4 sm:grid-cols-2">
			<div class="space-y-2">
				<Label>Backup</Label>
				{#if succeededBackups.length > 0}
					<Select.Root type="single" bind:value={restoreBackupId}>
						<Select.Trigger class="w-full text-xs">
							{succeededBackups.find((b) => b.id === restoreBackupId)?.created_at ?? 'Select backup'}
						</Select.Trigger>
						<Select.Content>
							{#each succeededBackups as b (b.id)}
								<Select.Item value={b.id} label={`${b.database_name} · ${b.created_at}`}>
									{b.database_name} · {b.created_at} ({fmtSize(b.size_bytes)})
								</Select.Item>
							{/each}
						</Select.Content>
					</Select.Root>
				{:else}
					<p class="text-sm text-muted-foreground">No successful backups available.</p>
				{/if}
			</div>
			<div class="space-y-2">
				<Label>Target database</Label>
				<Input bind:value={restoreTargetDb} class="font-mono" placeholder="restored_db" />
			</div>
		</div>
		<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
			<span>Clean target before restore</span>
			<Switch bind:checked={restoreClean} />
		</label>
		<div class="space-y-2">
			<Label>Confirm cluster name <span class="text-destructive">*</span></Label>
			<Input
				bind:value={restoreConfirmName}
				placeholder={cluster?.name ?? 'Type cluster name to confirm'}
			/>
		</div>
		<Button onclick={restore} disabled={restoring || succeededBackups.length === 0} variant="destructive">
			<HugeiconsIcon icon={RestoreBinIcon} class="size-4" strokeWidth={2} />
			{restoring ? 'Restoring…' : 'Restore backup'}
		</Button>
	</Card.Content>
</Card.Root>

<Card.Root class="border-border/60">
	<Card.Header>
		<Card.Title>History</Card.Title>
		<Card.Description>Recent logical backups for this cluster</Card.Description>
	</Card.Header>
	<Card.Content>
		{#if history.length === 0}
			<p class="text-sm text-muted-foreground">No backups yet — run one now.</p>
		{:else}
			<Table.Root>
				<Table.Header>
					<Table.Row>
						<Table.Head>When</Table.Head>
						<Table.Head>DB</Table.Head>
						<Table.Head>Kind</Table.Head>
						<Table.Head>Status</Table.Head>
						<Table.Head>Size</Table.Head>
						<Table.Head>Key</Table.Head>
					</Table.Row>
				</Table.Header>
				<Table.Body>
					{#each history as b (b.id)}
						<Table.Row>
							<Table.Cell class="text-xs whitespace-nowrap">{b.created_at}</Table.Cell>
							<Table.Cell class="font-mono text-xs">{b.database_name}</Table.Cell>
							<Table.Cell class="text-xs">{b.kind}</Table.Cell>
							<Table.Cell><StatusBadge status={b.status} /></Table.Cell>
							<Table.Cell class="tabular-nums text-xs">{fmtSize(b.size_bytes)}</Table.Cell>
							<Table.Cell class="max-w-[180px] truncate font-mono text-xs" title={b.storage_key ?? ''}
								>{b.storage_key ?? '—'}</Table.Cell
							>
						</Table.Row>
					{/each}
				</Table.Body>
			</Table.Root>
		{/if}
	</Card.Content>
</Card.Root>

<ConfirmDialog
	bind:open={deleteScheduleOpen}
	title="Delete backup schedule?"
	description="This schedule will be removed permanently."
	confirmLabel="Delete"
	variant="destructive"
	onConfirm={confirmDeleteSchedule}
/>

<ConfirmDialog
	bind:open={deleteTargetOpen}
	title="Remove backup target?"
	description="This destination binding will be removed from the cluster."
	confirmLabel="Remove"
	variant="destructive"
	onConfirm={confirmDeleteTarget}
/>
