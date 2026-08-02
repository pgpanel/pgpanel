<script lang="ts">
	import { onMount } from 'svelte';
	import {
		api,
		formatRelative,
		normalizeWalStatus,
		type WalSegment,
		type WalStreamStatus,
		type WalSegmentsResponse
	} from '$lib/api';
	import { trackOperation } from '$lib/jobs';
	import StatusBadge from '$lib/components/status-badge.svelte';
	import ConfirmDialog from '$lib/components/confirm-dialog.svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Input } from '$lib/components/ui/input/index.js';
	import { Label } from '$lib/components/ui/label/index.js';
	import { Switch } from '$lib/components/ui/switch/index.js';
	import * as Card from '$lib/components/ui/card/index.js';
	import * as Alert from '$lib/components/ui/alert/index.js';
	import * as Table from '$lib/components/ui/table/index.js';
	import { Separator } from '$lib/components/ui/separator/index.js';
	import { toast } from 'svelte-sonner';
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import {
		Alert02Icon,
		CloudBackupIcon,
		DatabaseSyncIcon,
		Delete02Icon,
		Download04Icon,
		InformationCircleIcon,
		RefreshIcon,
		RestoreBinIcon,
		Search01Icon,
		SecurityCheckIcon
	} from '@hugeicons/core-free-icons';

	let {
		clusterId,
		clusterName
	}: {
		clusterId: string;
		clusterName: string;
	} = $props();

	let wal = $state<WalStreamStatus | null>(null);
	let segments = $state<WalSegment[]>([]);
	let segmentTotal = $state(0);
	let error = $state('');
	let loading = $state(false);

	let searchQ = $state('');
	let searchFrom = $state('');
	let searchTo = $state('');
	let searchTimeline = $state('');
	let segmentLimit = $state(50);
	let segmentOffset = $state(0);

	let enableOpen = $state(false);
	let enableRetention = $state(14);
	let enableCompress = $state(true);
	let disableOpen = $state(false);
	let switchOpen = $state(false);
	let deleteSegmentOpen = $state(false);
	let deleteSegmentFilename = $state<string | null>(null);

	let pitrTargetTime = $state('');
	let pitrConfirmName = $state('');
	let pitrSubmitting = $state(false);

	const segmentPage = $derived(Math.floor(segmentOffset / segmentLimit) + 1);
	const segmentPageCount = $derived(
		segmentTotal > 0 ? Math.ceil(segmentTotal / segmentLimit) : 1
	);
	const pitrReady = $derived(
		pitrTargetTime.trim() !== '' && pitrConfirmName === clusterName
	);

	function fmtSize(n: number | null | undefined) {
		if (n == null) return '—';
		if (n < 1024) return `${n} B`;
		if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
		if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(2)} MB`;
		return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
	}

	function toIsoFromLocal(local: string): string | undefined {
		if (!local.trim()) return undefined;
		const d = new Date(local);
		if (Number.isNaN(d.getTime())) return undefined;
		return d.toISOString();
	}

	async function loadStatus() {
		const raw = await api<unknown>(`/api/clusters/${clusterId}/wal`);
		wal = normalizeWalStatus(raw);
	}

	async function loadSegments() {
		const params = new URLSearchParams();
		if (searchQ.trim()) params.set('q', searchQ.trim());
		const fromIso = toIsoFromLocal(searchFrom);
		const toIso = toIsoFromLocal(searchTo);
		if (fromIso) params.set('from', fromIso);
		if (toIso) params.set('to', toIso);
		if (searchTimeline.trim()) params.set('timeline', searchTimeline.trim());
		params.set('limit', String(segmentLimit));
		params.set('offset', String(segmentOffset));

		const res = await api<WalSegmentsResponse | WalSegment[]>(
			`/api/clusters/${clusterId}/wal/segments?${params}`
		);
		if (Array.isArray(res)) {
			segments = res;
			segmentTotal = res.length;
		} else {
			segments = res.segments ?? [];
			segmentTotal = res.total ?? segments.length;
		}
	}

	async function load() {
		loading = true;
		error = '';
		try {
			await loadStatus();
			if (wal?.searchable || wal?.enabled) {
				await loadSegments();
			} else {
				segments = [];
				segmentTotal = 0;
			}
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to load WAL status';
		} finally {
			loading = false;
		}
	}

	onMount(() => {
		load();
	});

	async function confirmEnable() {
		try {
			const res = await api<{ operation_id: string }>(
				`/api/clusters/${clusterId}/wal/enable`,
				{
					method: 'POST',
					body: JSON.stringify({
						retention_days: enableRetention,
						compress: enableCompress
					})
				}
			);
			trackOperation(res.operation_id, {
				title: 'Enable WAL archiving',
				onDone: () => load()
			});
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Enable failed');
			throw e;
		}
	}

	async function confirmDisable() {
		try {
			await api(`/api/clusters/${clusterId}/wal/disable`, { method: 'POST' });
			toast.success('WAL archiving disabled');
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Disable failed');
			throw e;
		}
	}

	async function syncNow() {
		error = '';
		try {
			const res = await api<{ operation_id: string }>(
				`/api/clusters/${clusterId}/wal/sync`,
				{ method: 'POST' }
			);
			trackOperation(res.operation_id, {
				title: 'Sync WAL segments',
				onDone: () => load()
			});
		} catch (e) {
			error = e instanceof Error ? e.message : 'Sync failed';
			toast.error(error);
		}
	}

	async function confirmSwitchWal() {
		try {
			const res = await api<{ operation_id: string }>(
				`/api/clusters/${clusterId}/wal/switch`,
				{ method: 'POST' }
			);
			trackOperation(res.operation_id, {
				title: 'Switch WAL file',
				onDone: () => load()
			});
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Switch failed');
			throw e;
		}
	}

	async function baseBackup() {
		error = '';
		try {
			const res = await api<{ operation_id: string }>(
				`/api/clusters/${clusterId}/wal/base-backup`,
				{ method: 'POST' }
			);
			trackOperation(res.operation_id, {
				title: 'Base backup',
				onDone: () => load()
			});
		} catch (e) {
			error = e instanceof Error ? e.message : 'Base backup failed';
			toast.error(error);
		}
	}

	async function searchSegments() {
		segmentOffset = 0;
		try {
			await loadSegments();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Search failed');
		}
	}

	async function prevPage() {
		if (segmentOffset <= 0) return;
		segmentOffset = Math.max(0, segmentOffset - segmentLimit);
		await loadSegments();
	}

	async function nextPage() {
		if (segmentOffset + segmentLimit >= segmentTotal) return;
		segmentOffset += segmentLimit;
		await loadSegments();
	}

	async function downloadSegment(filename: string) {
		try {
			const res = await fetch(
				`/api/clusters/${clusterId}/wal/segments/${encodeURIComponent(filename)}/download`,
				{ credentials: 'include' }
			);
			if (!res.ok) {
				throw new Error(`Download failed (${res.status})`);
			}
			const blob = await res.blob();
			const url = URL.createObjectURL(blob);
			const a = document.createElement('a');
			a.href = url;
			a.download = filename;
			a.click();
			URL.revokeObjectURL(url);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Download failed');
		}
	}

	function askDeleteSegment(filename: string) {
		deleteSegmentFilename = filename;
		deleteSegmentOpen = true;
	}

	async function confirmDeleteSegment() {
		if (!deleteSegmentFilename) return;
		try {
			await api(
				`/api/clusters/${clusterId}/wal/segments/${encodeURIComponent(deleteSegmentFilename)}`,
				{ method: 'DELETE' }
			);
			toast.success('Segment deleted');
			deleteSegmentFilename = null;
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Delete failed');
			throw e;
		}
	}

	async function submitPitr() {
		if (!pitrReady) {
			toast.error('Set target time and confirm cluster name');
			return;
		}
		const targetIso = toIsoFromLocal(pitrTargetTime);
		if (!targetIso) {
			toast.error('Invalid target time');
			return;
		}
		pitrSubmitting = true;
		try {
			const res = await api<{ operation_id: string }>(
				`/api/clusters/${clusterId}/wal/pitr`,
				{
					method: 'POST',
					body: JSON.stringify({
						target_time: targetIso,
						confirm_cluster_name: pitrConfirmName
					})
				}
			);
			trackOperation(res.operation_id, {
				title: 'Point-in-time recovery package',
				onDone: () => {
					toast.success(
						'PITR package ready — see backup history for the recovery manifest (base + WAL list + recovery.conf)'
					);
					load();
				}
			});
			pitrConfirmName = '';
			pitrTargetTime = '';
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'PITR failed');
		} finally {
			pitrSubmitting = false;
		}
	}
</script>

{#if error}
	<Alert.Root variant="destructive" class="mb-4">
		<HugeiconsIcon icon={Alert02Icon} class="size-4" strokeWidth={2} />
		<Alert.Description>{error}</Alert.Description>
	</Alert.Root>
{/if}

{#if !wal && loading}
	<p class="text-sm text-muted-foreground">Loading WAL status…</p>
{:else if wal}
	{#if !wal.enabled}
		<Alert.Root class="mb-4 border-border/60">
			<HugeiconsIcon icon={InformationCircleIcon} class="size-4" strokeWidth={2} />
			<Alert.Description>
				WAL archiving is not enabled for this cluster. Configure options below and enable archiving,
				then <strong>restart PostgreSQL</strong> so <code class="text-xs">archive_mode</code> takes
				effect. Until then, segments will not be collected and PITR will not be available.
			</Alert.Description>
		</Alert.Root>

		<Card.Root class="page-card mb-4 border-border/60">
			<Card.Header>
				<Card.Title>Enable options</Card.Title>
				<Card.Description>Applied when you enable WAL archiving</Card.Description>
			</Card.Header>
			<Card.Content class="grid gap-4 sm:grid-cols-2">
				<div class="space-y-2">
					<Label>Retention (days)</Label>
					<Input type="number" min="1" bind:value={enableRetention} />
				</div>
				<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
					<span>Compress segments</span>
					<Switch bind:checked={enableCompress} />
				</label>
			</Card.Content>
		</Card.Root>
	{/if}

	<div class="mb-4 flex flex-wrap gap-2">
		<Button variant="outline" size="sm" onclick={() => (enableOpen = true)} disabled={wal.enabled}>
			<HugeiconsIcon icon={SecurityCheckIcon} class="size-4" strokeWidth={2} />
			Enable WAL
		</Button>
		<Button
			variant="outline"
			size="sm"
			onclick={() => (disableOpen = true)}
			disabled={!wal.enabled}
		>
			Disable WAL
		</Button>
		<Button variant="outline" size="sm" onclick={syncNow} disabled={!wal.enabled}>
			<HugeiconsIcon icon={RefreshIcon} class="size-4" strokeWidth={2} />
			Sync now
		</Button>
		<Button
			variant="outline"
			size="sm"
			onclick={() => (switchOpen = true)}
			disabled={!wal.enabled}
		>
			<HugeiconsIcon icon={DatabaseSyncIcon} class="size-4" strokeWidth={2} />
			Switch WAL
		</Button>
		<Button variant="outline" size="sm" onclick={baseBackup} disabled={!wal.enabled}>
			<HugeiconsIcon icon={CloudBackupIcon} class="size-4" strokeWidth={2} />
			Base backup
		</Button>
		<Button variant="ghost" size="sm" onclick={() => load()} disabled={loading}>
			Refresh
		</Button>
	</div>

	<div class="mb-6 grid gap-4 lg:grid-cols-2">
		<Card.Root class="page-card border-border/60">
			<Card.Header class="flex-row items-center justify-between space-y-0">
				<Card.Title>WAL stream status</Card.Title>
				<div class="flex items-center gap-2">
					<StatusBadge status={wal.enabled ? 'enabled' : 'disabled'} />
					<StatusBadge status={wal.status} />
				</div>
			</Card.Header>
			<Card.Content class="space-y-3 text-sm">
				<div class="flex justify-between gap-4">
					<span class="text-muted-foreground">Archive directory</span>
					<span class="max-w-[240px] truncate font-mono text-right text-xs" title={wal.archive_dir ?? ''}>
						{wal.archive_dir ?? '—'}
					</span>
				</div>
				<Separator />
				<div class="flex justify-between gap-4">
					<span class="text-muted-foreground">Last segment</span>
					<span class="font-mono text-xs">{wal.last_segment ?? '—'}</span>
				</div>
				<Separator />
				<div class="flex justify-between gap-4">
					<span class="text-muted-foreground">Segments</span>
					<span class="tabular-nums">{wal.segment_count}</span>
				</div>
				<Separator />
				<div class="flex justify-between gap-4">
					<span class="text-muted-foreground">Total size</span>
					<span class="tabular-nums">{fmtSize(wal.total_bytes)}</span>
				</div>
				<Separator />
				<div class="flex justify-between gap-4">
					<span class="text-muted-foreground">Timeline</span>
					<span class="font-mono text-xs">{wal.timeline ?? '—'}</span>
				</div>
				<Separator />
				<div class="flex justify-between gap-4">
					<span class="text-muted-foreground">Last synced</span>
					<span class="text-right text-xs">
						{wal.last_synced_at ? formatRelative(wal.last_synced_at) : '—'}
					</span>
				</div>
				<Separator />
				<div class="flex justify-between gap-4">
					<span class="text-muted-foreground">Retention</span>
					<span>{wal.retention_days} days</span>
				</div>
			</Card.Content>
		</Card.Root>

		<Card.Root class="page-card border-border/60">
			<Card.Header>
				<Card.Title>PostgreSQL archiving</Card.Title>
				<Card.Description>Live settings from the database</Card.Description>
			</Card.Header>
			<Card.Content class="space-y-3 text-sm">
				<div class="flex justify-between gap-4">
					<span class="text-muted-foreground">Archive mode</span>
					<span class="font-mono text-xs">{wal.pg.archive_mode ?? '—'}</span>
				</div>
				<Separator />
				<div class="flex justify-between gap-4">
					<span class="text-muted-foreground">WAL level</span>
					<span class="font-mono text-xs">{wal.pg.wal_level ?? '—'}</span>
				</div>
				<Separator />
				<div class="flex justify-between gap-4">
					<span class="text-muted-foreground">Last archived WAL</span>
					<span class="font-mono text-xs">{wal.pg.last_archived_wal ?? '—'}</span>
				</div>
				<Separator />
				<div class="flex justify-between gap-4">
					<span class="text-muted-foreground">Archive failures</span>
					<span class="tabular-nums">{wal.pg.failed_count}</span>
				</div>
				{#if wal.pg.message}
					<p class="pt-2 text-muted-foreground">{wal.pg.message}</p>
				{/if}
			</Card.Content>
		</Card.Root>
	</div>

	<Card.Root class="page-card mb-6 border-border/60">
		<Card.Header>
			<Card.Title class="flex items-center gap-2">
				<HugeiconsIcon icon={Search01Icon} class="size-4" strokeWidth={2} />
				Search & retrieve segments
			</Card.Title>
			<Card.Description>
				Filter archived WAL segments by filename, time range, or timeline
			</Card.Description>
		</Card.Header>
		<Card.Content class="space-y-4">
			{#if !wal.enabled}
				<p class="text-sm text-muted-foreground">
					Enable WAL archiving and restart the cluster to start collecting segments.
				</p>
			{:else if !wal.searchable}
				<p class="text-sm text-muted-foreground">
					Segment index is not ready yet — run <strong>Sync now</strong> after archiving is active.
				</p>
			{:else}
				<form
					class="grid gap-4 sm:grid-cols-2 lg:grid-cols-4"
					onsubmit={(e) => {
						e.preventDefault();
						searchSegments();
					}}
				>
					<div class="space-y-2">
						<Label>Filename search</Label>
						<Input bind:value={searchQ} class="font-mono text-xs" placeholder="0000000100000000" />
					</div>
					<div class="space-y-2">
						<Label>From</Label>
						<Input type="datetime-local" bind:value={searchFrom} />
					</div>
					<div class="space-y-2">
						<Label>To</Label>
						<Input type="datetime-local" bind:value={searchTo} />
					</div>
					<div class="space-y-2">
						<Label>Timeline</Label>
						<Input bind:value={searchTimeline} class="font-mono text-xs" placeholder="1" />
					</div>
				</form>
				<div class="flex flex-wrap gap-2">
					<Button size="sm" onclick={searchSegments}>
						<HugeiconsIcon icon={Search01Icon} class="size-4" strokeWidth={2} />
						Search
					</Button>
					<Button
						size="sm"
						variant="outline"
						onclick={() => {
							searchQ = '';
							searchFrom = '';
							searchTo = '';
							searchTimeline = '';
							segmentOffset = 0;
							loadSegments();
						}}
					>
						Clear filters
					</Button>
				</div>

				{#if segments.length === 0}
					<p class="text-sm text-muted-foreground">No segments match your filters.</p>
				{:else}
					<Table.Root>
						<Table.Header>
							<Table.Row class="hover:bg-transparent">
								<Table.Head>Filename</Table.Head>
								<Table.Head>Timeline</Table.Head>
								<Table.Head>Size</Table.Head>
								<Table.Head>Archived</Table.Head>
								<Table.Head>Synced</Table.Head>
								<Table.Head></Table.Head>
							</Table.Row>
						</Table.Header>
						<Table.Body>
							{#each segments as seg (seg.id)}
								<Table.Row>
									<Table.Cell class="font-mono text-xs">{seg.filename}</Table.Cell>
									<Table.Cell class="font-mono text-xs">{seg.timeline}</Table.Cell>
									<Table.Cell class="tabular-nums text-xs">{fmtSize(seg.size_bytes)}</Table.Cell>
									<Table.Cell class="text-xs whitespace-nowrap">
										{seg.archived_at ? formatRelative(seg.archived_at) : '—'}
									</Table.Cell>
									<Table.Cell class="text-xs whitespace-nowrap">
										{seg.synced_at ? formatRelative(seg.synced_at) : '—'}
									</Table.Cell>
									<Table.Cell class="flex gap-1">
										<Button
											variant="ghost"
											size="sm"
											onclick={() => downloadSegment(seg.filename)}
											title="Download"
										>
											<HugeiconsIcon icon={Download04Icon} class="size-4" strokeWidth={2} />
										</Button>
										<Button
											variant="ghost"
											size="sm"
											onclick={() => askDeleteSegment(seg.filename)}
											title="Delete"
										>
											<HugeiconsIcon icon={Delete02Icon} class="size-4" strokeWidth={2} />
										</Button>
									</Table.Cell>
								</Table.Row>
							{/each}
						</Table.Body>
					</Table.Root>
					<div class="flex items-center justify-between text-sm text-muted-foreground">
						<span>
							{segmentTotal} segment{segmentTotal === 1 ? '' : 's'} · page {segmentPage} of
							{segmentPageCount}
						</span>
						<div class="flex gap-2">
							<Button
								size="sm"
								variant="outline"
								disabled={segmentOffset <= 0}
								onclick={prevPage}
							>
								Previous
							</Button>
							<Button
								size="sm"
								variant="outline"
								disabled={segmentOffset + segmentLimit >= segmentTotal}
								onclick={nextPage}
							>
								Next
							</Button>
						</div>
					</div>
				{/if}
			{/if}
		</Card.Content>
	</Card.Root>

	<Card.Root class="page-card border-border/60">
		<Card.Header>
			<Card.Title class="flex items-center gap-2">
				<HugeiconsIcon icon={RestoreBinIcon} class="size-4" strokeWidth={2} />
				Point-in-time recovery (PITR)
			</Card.Title>
			<Card.Description>
				Restore the cluster to a specific timestamp using base backups and WAL segments. This is
				destructive.
			</Card.Description>
		</Card.Header>
		<Card.Content class="space-y-4">
			{#if !wal.enabled}
				<p class="text-sm text-muted-foreground">
					PITR requires WAL archiving, a base backup, and continuous segment collection. Enable WAL
					first, restart PostgreSQL, then take a base backup.
				</p>
			{:else}
				<Alert.Root class="border-destructive/40 bg-destructive/5">
					<HugeiconsIcon icon={Alert02Icon} class="size-4" strokeWidth={2} />
					<Alert.Description>
						PITR will replace the current database state. Ensure you have a recent base backup and
						all required WAL segments before proceeding.
					</Alert.Description>
				</Alert.Root>
				<div class="grid gap-4 sm:grid-cols-2">
					<div class="space-y-2">
						<Label>Target time</Label>
						<Input type="datetime-local" bind:value={pitrTargetTime} required />
					</div>
					<div class="space-y-2">
						<Label>Confirm cluster name <span class="text-destructive">*</span></Label>
						<Input
							bind:value={pitrConfirmName}
							placeholder={clusterName}
							autocomplete="off"
						/>
					</div>
				</div>
				<Button
					variant="destructive"
					onclick={submitPitr}
					disabled={!pitrReady || pitrSubmitting}
				>
					<HugeiconsIcon icon={RestoreBinIcon} class="size-4" strokeWidth={2} />
					{pitrSubmitting ? 'Starting PITR…' : 'Start point-in-time recovery'}
				</Button>
			{/if}
		</Card.Content>
	</Card.Root>
{/if}

<ConfirmDialog
	bind:open={enableOpen}
	title="Enable WAL archiving?"
	description="PostgreSQL will be configured for continuous archiving. A cluster restart is required before archive_mode becomes active."
	confirmLabel="Enable"
	onConfirm={confirmEnable}
/>

<ConfirmDialog
	bind:open={disableOpen}
	title="Disable WAL archiving?"
	description="New WAL segments will no longer be archived. Existing segments remain until retention prunes them."
	confirmLabel="Disable"
	variant="destructive"
	onConfirm={confirmDisable}
/>

<ConfirmDialog
	bind:open={switchOpen}
	title="Force WAL segment switch?"
	description="Runs pg_switch_wal() to archive the current WAL file. Use when you need an immediate segment boundary."
	confirmLabel="Switch WAL"
	onConfirm={confirmSwitchWal}
/>

<ConfirmDialog
	bind:open={deleteSegmentOpen}
	title="Delete WAL segment?"
	description={deleteSegmentFilename
		? `Remove ${deleteSegmentFilename} from the archive. PITR to times requiring this segment may fail.`
		: ''}
	confirmLabel="Delete"
	variant="destructive"
	onConfirm={confirmDeleteSegment}
/>
