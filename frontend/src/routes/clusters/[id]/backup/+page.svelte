<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/stores';
	import {
		api,
		type BackupSchedule,
		type Cluster,
		type DatabaseRecord,
		type BackupDestination,
		type ClusterBackupTarget
	} from '$lib/api';
	import { trackOperation } from '$lib/jobs';
	import PageHeader from '$lib/components/page-header.svelte';
	import StatusBadge from '$lib/components/status-badge.svelte';
	import ConfirmDialog from '$lib/components/confirm-dialog.svelte';
	import ClusterWalPanel from '$lib/components/cluster-wal-panel.svelte';
	import DatabaseSelect from '$lib/components/database-select.svelte';
	import SearchableSelect, { type SearchableOption } from '$lib/components/searchable-select.svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Input } from '$lib/components/ui/input/index.js';
	import { Label } from '$lib/components/ui/label/index.js';
	import { Switch } from '$lib/components/ui/switch/index.js';
	import * as Card from '$lib/components/ui/card/index.js';
	import * as Alert from '$lib/components/ui/alert/index.js';
	import * as Table from '$lib/components/ui/table/index.js';
	import * as Select from '$lib/components/ui/select/index.js';
	import * as Tabs from '$lib/components/ui/tabs/index.js';
	import * as Dialog from '$lib/components/ui/dialog/index.js';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu/index.js';
	import { toast } from 'svelte-sonner';
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import {
		Alert02Icon,
		RefreshIcon,
		CloudBackupIcon,
		Calendar03Icon,
		Target01Icon,
		DatabaseSyncIcon,
		Delete02Icon,
		RestoreBinIcon,
		Add01Icon,
		ArrowDown01Icon
	} from '@hugeicons/core-free-icons';

	interface BackupStatus {
		integration_status: string;
		last_successful_backup: string | null;
		last_backup_status: string | null;
		backup_lag_seconds: number | null;
		wal_status: string | null;
		failed_backups: number;
		message: string | null;
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

	const ALL_DATABASES = '*';

	const id = $derived($page.params.id);
	let activeTab = $state('logical');
	let cluster = $state<Cluster | null>(null);
	let status = $state<BackupStatus | null>(null);
	let history = $state<BackupRow[]>([]);
	let schedules = $state<BackupSchedule[]>([]);
	let destinations = $state<BackupDestination[]>([]);
	let targets = $state<ClusterBackupTarget[]>([]);
	let error = $state('');
	let refreshing = $state(false);

	// Backup now dialog (single database)
	let backupNowOpen = $state(false);
	let backupDatabase = $state('postgres');
	let backupSchemaOnly = $state(false);
	let backupTriggering = $state(false);

	// Add schedule dialog
	let addScheduleOpen = $state(false);
	let scheduleCron = $state('0 3 * * *');
	let scheduleDatabase = $state(ALL_DATABASES);
	let scheduleRetention = $state(14);
	let scheduleKeepCount = $state(30);
	let scheduleSaving = $state(false);
	let scheduleDbOptions = $state<SearchableOption[]>([
		{ value: ALL_DATABASES, label: 'All databases (full cluster)' }
	]);

	// Restore dialog
	let restoreOpen = $state(false);
	let restoreBackup = $state<BackupRow | null>(null);
	let restoreTargetDb = $state('');
	let restoreClean = $state(false);
	let restoreConfirmName = $state('');
	let restoring = $state(false);

	// Add target dialog
	let addTargetOpen = $state(false);
	let targetDestinationId = $state('');
	let targetCron = $state('0 3 * * *');
	let targetRetention = $state(14);
	let targetKeepCount = $state(30);
	let targetEnabled = $state(true);
	let targetSaving = $state(false);

	// Delete confirmations
	let deleteScheduleOpen = $state(false);
	let deleteScheduleId = $state<string | null>(null);
	let deleteTargetOpen = $state(false);
	let deleteTargetId = $state<string | null>(null);

	function isRestorable(backup: BackupRow) {
		return backup.status === 'succeeded' || backup.status === 'success';
	}

	async function loadScheduleDbOptions() {
		try {
			const databases = await api<DatabaseRecord[]>(`/api/clusters/${id}/databases`);
			scheduleDbOptions = [
				{ value: ALL_DATABASES, label: 'All databases (full cluster)' },
				...databases.map((db) => ({ value: db.name, label: db.name }))
			];
		} catch {
			scheduleDbOptions = [{ value: ALL_DATABASES, label: 'All databases (full cluster)' }];
		}
	}

	async function load() {
		const [st, h, sch, cl, dests, tgts] = await Promise.all([
			api<BackupStatus>(`/api/clusters/${id}/backup`),
			api<{ backups: BackupRow[] }>(`/api/clusters/${id}/backup/history`),
			api<BackupSchedule[]>(`/api/clusters/${id}/backup/schedules`),
			api<Cluster>(`/api/clusters/${id}`),
			api<BackupDestination[]>('/api/backup-destinations').catch(() => [] as BackupDestination[]),
			api<ClusterBackupTarget[]>(`/api/clusters/${id}/backup-targets`).catch(
				() => [] as ClusterBackupTarget[]
			)
		]);
		status = st;
		history = h.backups ?? [];
		schedules = sch;
		cluster = cl;
		destinations = dests;
		targets = tgts;
		if (!targetDestinationId && dests.length > 0) {
			targetDestinationId = dests.find((d) => d.is_default)?.id ?? dests[0].id;
		}
	}

	onMount(() => {
		const tab = $page.url.searchParams.get('tab');
		if (tab === 'wal') activeTab = 'wal';
		load().catch((e) => (error = e instanceof Error ? e.message : 'Failed to load'));
		loadScheduleDbOptions();
	});

	async function refresh() {
		refreshing = true;
		error = '';
		try {
			await load();
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to refresh';
			toast.error(error);
		} finally {
			refreshing = false;
		}
	}

	async function enableBackups() {
		error = '';
		try {
			const res = await api<{ operation_id: string }>(`/api/clusters/${id}/backup/enable`, {
				method: 'POST'
			});
			trackOperation(res.operation_id, {
				title: 'Enable backups',
				onDone: () => load()
			});
			toast.success('Enabling backups…');
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed';
			toast.error(error);
		}
	}

	async function triggerBackup(database: string, schemaOnly = false) {
		error = '';
		backupTriggering = true;
		try {
			const res = await api<{ operation_id: string }>(`/api/clusters/${id}/backup/trigger`, {
				method: 'POST',
				body: JSON.stringify({ database, schema_only: schemaOnly })
			});
			trackOperation(res.operation_id, {
				title: database === ALL_DATABASES ? 'Full cluster backup' : 'Logical backup',
				onDone: () => load(),
				onFail: () => load()
			});
			toast.success(
				database === ALL_DATABASES ? 'Full cluster backup started' : `Backup started for ${database}`
			);
			backupNowOpen = false;
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed';
			toast.error(error);
		} finally {
			backupTriggering = false;
		}
	}

	function openBackupNowDialog() {
		backupDatabase = 'postgres';
		backupSchemaOnly = false;
		backupNowOpen = true;
	}

	function openAddScheduleDialog() {
		scheduleCron = '0 3 * * *';
		scheduleDatabase = ALL_DATABASES;
		scheduleRetention = 14;
		scheduleKeepCount = 30;
		loadScheduleDbOptions();
		addScheduleOpen = true;
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
					enabled: true,
					retention_days: scheduleRetention,
					keep_count: scheduleKeepCount
				})
			});
			toast.success('Schedule added');
			addScheduleOpen = false;
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

	function openRestoreDialog(backup: BackupRow) {
		restoreBackup = backup;
		restoreTargetDb = backup.database_name === ALL_DATABASES ? '' : backup.database_name;
		restoreClean = false;
		restoreConfirmName = '';
		restoreOpen = true;
	}

	async function confirmRestore(e: Event) {
		e.preventDefault();
		if (!restoreBackup || !restoreTargetDb.trim()) {
			toast.error('Enter a target database name');
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
					backup_id: restoreBackup.id,
					target_database: restoreTargetDb,
					clean: restoreClean,
					confirm_cluster_name: restoreConfirmName
				})
			});
			trackOperation(res.operation_id, {
				title: 'Restore backup',
				onDone: () => load()
			});
			toast.success('Restore started');
			restoreOpen = false;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Restore failed');
		} finally {
			restoring = false;
		}
	}

	function openAddTargetDialog() {
		if (destinations.length > 0 && !targetDestinationId) {
			targetDestinationId = destinations.find((d) => d.is_default)?.id ?? destinations[0].id;
		}
		targetCron = '0 3 * * *';
		targetRetention = 14;
		targetKeepCount = 30;
		targetEnabled = true;
		addTargetOpen = true;
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
					keep_count: targetKeepCount
				})
			});
			toast.success('Destination bound');
			addTargetOpen = false;
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to bind destination');
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
			toast.success('Destination unbound');
			deleteTargetId = null;
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Delete failed');
		}
	}

	function fmtSize(n: number | null) {
		if (n == null) return '—';
		if (n < 1024) return `${n} B`;
		if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
		return `${(n / 1024 / 1024).toFixed(2)} MB`;
	}

	function fmtDatabase(name: string) {
		return name === ALL_DATABASES ? 'All databases' : name;
	}
</script>

<PageHeader title="Backups" description="Logical dumps and point-in-time recovery">
	{#snippet actions()}
		{#if activeTab === 'logical'}
			<Button variant="outline" onclick={enableBackups}>
				<HugeiconsIcon icon={CloudBackupIcon} class="size-4" strokeWidth={2} />
				Enable backups
			</Button>
			<DropdownMenu.Root>
				<DropdownMenu.Trigger>
					{#snippet child({ props })}
						<Button {...props}>
							<HugeiconsIcon icon={CloudBackupIcon} class="size-4" strokeWidth={2} />
							Backup now
							<HugeiconsIcon icon={ArrowDown01Icon} class="size-4 opacity-60" strokeWidth={2} />
						</Button>
					{/snippet}
				</DropdownMenu.Trigger>
				<DropdownMenu.Content align="end">
					<DropdownMenu.Item onclick={() => triggerBackup(ALL_DATABASES)}>
						All databases (full cluster)
					</DropdownMenu.Item>
					<DropdownMenu.Item onclick={openBackupNowDialog}>One database…</DropdownMenu.Item>
				</DropdownMenu.Content>
			</DropdownMenu.Root>
		{/if}
		<Button variant="outline" onclick={refresh} disabled={refreshing}>
			<HugeiconsIcon icon={RefreshIcon} class="size-4" strokeWidth={2} />
			{refreshing ? 'Refreshing…' : 'Refresh'}
		</Button>
	{/snippet}
</PageHeader>

<Tabs.Root bind:value={activeTab} class="mb-6 space-y-6">
	<Tabs.List>
		<Tabs.Trigger value="logical">
			<HugeiconsIcon icon={CloudBackupIcon} class="size-4" strokeWidth={2} />
			Logical
		</Tabs.Trigger>
		<Tabs.Trigger value="wal">
			<HugeiconsIcon icon={DatabaseSyncIcon} class="size-4" strokeWidth={2} />
			WAL / PITR
		</Tabs.Trigger>
	</Tabs.List>

	<Tabs.Content value="logical" class="space-y-6">
		{#if error}
			<Alert.Root variant="destructive">
				<HugeiconsIcon icon={Alert02Icon} class="size-4" strokeWidth={2} />
				<Alert.Description>{error}</Alert.Description>
			</Alert.Root>
		{/if}

		{#if status}
			<Card.Root class="border-border/60">
				<Card.Content class="flex flex-wrap items-center gap-x-6 gap-y-3 py-4 text-sm">
					<div class="flex items-center gap-2">
						<span class="text-muted-foreground">Status</span>
						<StatusBadge status={status.integration_status} />
					</div>
					<div class="flex items-center gap-2">
						<span class="text-muted-foreground">Last success</span>
						<span class="text-xs tabular-nums">{status.last_successful_backup ?? '—'}</span>
					</div>
					<div class="flex items-center gap-2">
						<span class="text-muted-foreground">Failed</span>
						<span class="tabular-nums font-medium">{status.failed_backups}</span>
					</div>
					{#if status.message}
						<span class="text-muted-foreground">{status.message}</span>
					{/if}
				</Card.Content>
			</Card.Root>
		{/if}

		<Card.Root class="border-border/60">
			<Card.Header>
				<Card.Title>History</Card.Title>
				<Card.Description>Recent logical backups for this cluster</Card.Description>
			</Card.Header>
			<Card.Content>
				{#if history.length === 0}
					<p class="text-sm text-muted-foreground">No backups yet — run one with Backup now.</p>
				{:else}
					<Table.Root>
						<Table.Header>
							<Table.Row class="hover:bg-transparent">
								<Table.Head>When</Table.Head>
								<Table.Head>Database</Table.Head>
								<Table.Head>Kind</Table.Head>
								<Table.Head>Status</Table.Head>
								<Table.Head>Size</Table.Head>
								<Table.Head class="w-[100px]"></Table.Head>
							</Table.Row>
						</Table.Header>
						<Table.Body>
							{#each history as b (b.id)}
								<Table.Row>
									<Table.Cell class="whitespace-nowrap text-xs">{b.created_at}</Table.Cell>
									<Table.Cell class="font-mono text-xs">{fmtDatabase(b.database_name)}</Table.Cell>
									<Table.Cell class="text-xs">{b.kind}</Table.Cell>
									<Table.Cell><StatusBadge status={b.status} /></Table.Cell>
									<Table.Cell class="tabular-nums text-xs">{fmtSize(b.size_bytes)}</Table.Cell>
									<Table.Cell>
										{#if isRestorable(b)}
											<Button variant="ghost" size="sm" onclick={() => openRestoreDialog(b)}>
												<HugeiconsIcon icon={RestoreBinIcon} class="size-4" strokeWidth={2} />
												Restore
											</Button>
										{/if}
									</Table.Cell>
								</Table.Row>
							{/each}
						</Table.Body>
					</Table.Root>
				{/if}
			</Card.Content>
		</Card.Root>

		<Card.Root class="border-border/60">
			<Card.Header class="flex-row items-center justify-between space-y-0">
				<div class="space-y-1">
					<Card.Title class="flex items-center gap-2">
						<HugeiconsIcon icon={Calendar03Icon} class="size-4" strokeWidth={2} />
						Schedules
					</Card.Title>
					<Card.Description>Automated backup cron jobs</Card.Description>
				</div>
				<Button variant="outline" size="sm" onclick={openAddScheduleDialog}>
					<HugeiconsIcon icon={Add01Icon} class="size-4" strokeWidth={2} />
					Add schedule
				</Button>
			</Card.Header>
			<Card.Content>
				{#if schedules.length === 0}
					<p class="text-sm text-muted-foreground">No schedules — add one to run backups automatically.</p>
				{:else}
					<Table.Root>
						<Table.Header>
							<Table.Row class="hover:bg-transparent">
								<Table.Head>Database</Table.Head>
								<Table.Head>Cron</Table.Head>
								<Table.Head>Retention</Table.Head>
								<Table.Head>Enabled</Table.Head>
								<Table.Head>Next run</Table.Head>
								<Table.Head class="w-[60px]"></Table.Head>
							</Table.Row>
						</Table.Header>
						<Table.Body>
							{#each schedules as s (s.id)}
								<Table.Row>
									<Table.Cell class="font-mono text-xs">{fmtDatabase(s.database_name)}</Table.Cell>
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
				{/if}
			</Card.Content>
		</Card.Root>

		<Card.Root class="border-border/60">
			<Card.Header class="flex-row items-center justify-between space-y-0">
				<div class="space-y-1">
					<Card.Title class="flex items-center gap-2">
						<HugeiconsIcon icon={Target01Icon} class="size-4" strokeWidth={2} />
						Destinations
					</Card.Title>
					<Card.Description>Where backups for this cluster are stored</Card.Description>
				</div>
				{#if destinations.length > 0}
					<Button variant="outline" size="sm" onclick={openAddTargetDialog}>
						<HugeiconsIcon icon={Add01Icon} class="size-4" strokeWidth={2} />
						Bind destination
					</Button>
				{/if}
			</Card.Header>
			<Card.Content>
				{#if destinations.length === 0}
					<p class="text-sm text-muted-foreground">
						No backup destinations configured.
						<a href="/destinations" class="text-primary hover:underline">Configure destinations →</a>
					</p>
				{:else if targets.length === 0}
					<p class="text-sm text-muted-foreground">
						No destinations bound to this cluster.
						<button type="button" class="text-primary hover:underline" onclick={openAddTargetDialog}>
							Bind one now
						</button>
						or
						<a href="/destinations" class="text-primary hover:underline">manage destinations →</a>
					</p>
				{:else}
					<Table.Root>
						<Table.Header>
							<Table.Row class="hover:bg-transparent">
								<Table.Head>Destination</Table.Head>
								<Table.Head>Cron</Table.Head>
								<Table.Head>Retention</Table.Head>
								<Table.Head>Status</Table.Head>
								<Table.Head class="w-[60px]"></Table.Head>
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
				{/if}
			</Card.Content>
		</Card.Root>
	</Tabs.Content>

	<Tabs.Content value="wal">
		{#if cluster && id}
			<ClusterWalPanel clusterId={id} clusterName={cluster.name} />
		{:else}
			<p class="text-sm text-muted-foreground">Loading cluster…</p>
		{/if}
	</Tabs.Content>
</Tabs.Root>

<!-- Backup now (single database) -->
<Dialog.Root bind:open={backupNowOpen}>
	<Dialog.Content class="sm:max-w-md">
		<Dialog.Header>
			<Dialog.Title>Backup one database</Dialog.Title>
			<Dialog.Description>Run an on-demand logical dump for a single database.</Dialog.Description>
		</Dialog.Header>
		<form
			class="space-y-4"
			onsubmit={(e) => {
				e.preventDefault();
				triggerBackup(backupDatabase, backupSchemaOnly);
			}}
		>
			<div class="space-y-2">
				<Label>Database</Label>
				<DatabaseSelect clusterId={id} bind:value={backupDatabase} />
			</div>
			<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
				<span>Schema only</span>
				<Switch bind:checked={backupSchemaOnly} />
			</label>
			<Dialog.Footer class="gap-2 border-t-0 bg-transparent sm:justify-end">
				<Button type="button" variant="outline" onclick={() => (backupNowOpen = false)}>Cancel</Button>
				<Button type="submit" disabled={backupTriggering}>
					{backupTriggering ? 'Starting…' : 'Run backup'}
				</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>

<!-- Add schedule -->
<Dialog.Root bind:open={addScheduleOpen}>
	<Dialog.Content class="sm:max-w-md">
		<Dialog.Header>
			<Dialog.Title>Add schedule</Dialog.Title>
			<Dialog.Description>Create an automated backup schedule for this cluster.</Dialog.Description>
		</Dialog.Header>
		<form class="space-y-4" onsubmit={saveSchedule}>
			<div class="space-y-2">
				<Label for="schedule-cron">Cron expression</Label>
				<Input id="schedule-cron" bind:value={scheduleCron} class="font-mono" placeholder="0 3 * * *" required />
			</div>
			<div class="space-y-2">
				<Label>Database</Label>
				<SearchableSelect bind:value={scheduleDatabase} items={scheduleDbOptions} mono />
			</div>
			<div class="grid gap-4 sm:grid-cols-2">
				<div class="space-y-2">
					<Label for="schedule-retention">Retention (days)</Label>
					<Input id="schedule-retention" type="number" min="1" bind:value={scheduleRetention} />
				</div>
				<div class="space-y-2">
					<Label for="schedule-keep">Keep count</Label>
					<Input id="schedule-keep" type="number" min="1" bind:value={scheduleKeepCount} />
				</div>
			</div>
			<Dialog.Footer class="gap-2 border-t-0 bg-transparent sm:justify-end">
				<Button type="button" variant="outline" onclick={() => (addScheduleOpen = false)}>Cancel</Button>
				<Button type="submit" disabled={scheduleSaving}>
					{scheduleSaving ? 'Saving…' : 'Add schedule'}
				</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>

<!-- Restore -->
<Dialog.Root bind:open={restoreOpen}>
	<Dialog.Content class="sm:max-w-md">
		<Dialog.Header>
			<Dialog.Title>Restore backup</Dialog.Title>
			<Dialog.Description>
				{#if restoreBackup}
					Restore <span class="font-mono">{fmtDatabase(restoreBackup.database_name)}</span> from
					{restoreBackup.created_at} into a target database.
				{:else}
					Restore a backup into a target database.
				{/if}
			</Dialog.Description>
		</Dialog.Header>
		{#if restoreBackup}
			<form class="space-y-4" onsubmit={confirmRestore}>
				<div class="space-y-2">
					<Label for="restore-target">Target database</Label>
					<Input
						id="restore-target"
						bind:value={restoreTargetDb}
						class="font-mono"
						placeholder="restored_db"
						required
					/>
				</div>
				<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
					<span>Clean target before restore</span>
					<Switch bind:checked={restoreClean} />
				</label>
				<div class="space-y-2">
					<Label for="restore-confirm">
						Confirm cluster name <span class="text-destructive">*</span>
					</Label>
					<Input
						id="restore-confirm"
						bind:value={restoreConfirmName}
						placeholder={cluster?.name ?? 'Type cluster name to confirm'}
						required
					/>
				</div>
				<Dialog.Footer class="gap-2 border-t-0 bg-transparent sm:justify-end">
					<Button type="button" variant="outline" onclick={() => (restoreOpen = false)}>Cancel</Button>
					<Button type="submit" variant="destructive" disabled={restoring}>
						{restoring ? 'Restoring…' : 'Restore'}
					</Button>
				</Dialog.Footer>
			</form>
		{/if}
	</Dialog.Content>
</Dialog.Root>

<!-- Bind destination -->
<Dialog.Root bind:open={addTargetOpen}>
	<Dialog.Content class="sm:max-w-md">
		<Dialog.Header>
			<Dialog.Title>Bind destination</Dialog.Title>
			<Dialog.Description>Link a storage destination to this cluster.</Dialog.Description>
		</Dialog.Header>
		<form class="space-y-4" onsubmit={saveTarget}>
			<div class="space-y-2">
				<Label>Destination</Label>
				<Select.Root type="single" bind:value={targetDestinationId}>
					<Select.Trigger class="w-full">
						{destinations.find((d) => d.id === targetDestinationId)?.name ?? 'Select destination'}
					</Select.Trigger>
					<Select.Content>
						{#each destinations as d (d.id)}
							<Select.Item value={d.id} label={d.name}>{d.name}</Select.Item>
						{/each}
					</Select.Content>
				</Select.Root>
			</div>
			<div class="space-y-2">
				<Label for="target-cron">Cron</Label>
				<Input id="target-cron" bind:value={targetCron} class="font-mono" required />
			</div>
			<div class="grid gap-4 sm:grid-cols-2">
				<div class="space-y-2">
					<Label for="target-retention">Retention (days)</Label>
					<Input id="target-retention" type="number" min="1" bind:value={targetRetention} />
				</div>
				<div class="space-y-2">
					<Label for="target-keep">Keep count</Label>
					<Input id="target-keep" type="number" min="1" bind:value={targetKeepCount} />
				</div>
			</div>
			<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
				<span>Enabled</span>
				<Switch bind:checked={targetEnabled} />
			</label>
			<Dialog.Footer class="gap-2 border-t-0 bg-transparent sm:justify-end">
				<Button type="button" variant="outline" onclick={() => (addTargetOpen = false)}>Cancel</Button>
				<Button type="submit" disabled={targetSaving}>
					{targetSaving ? 'Saving…' : 'Bind destination'}
				</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>

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
	title="Remove destination binding?"
	description="This destination will no longer be used for this cluster."
	confirmLabel="Remove"
	variant="destructive"
	onConfirm={confirmDeleteTarget}
/>
