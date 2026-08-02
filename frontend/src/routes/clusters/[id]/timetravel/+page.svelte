<script lang="ts">
	import { page } from '$app/stores';
	import { goto } from '$app/navigation';
	import {
		api,
		formatRelative,
		normalizeWalStatus,
		type Cluster,
		type WalSegment,
		type WalSegmentsResponse,
		type WalStreamStatus
	} from '$lib/api';
	import { trackOperation } from '$lib/jobs';
	import PageHeader from '$lib/components/page-header.svelte';
	import ClusterSelect from '$lib/components/cluster-select.svelte';
	import StatusBadge from '$lib/components/status-badge.svelte';
	import ConfirmDialog from '$lib/components/confirm-dialog.svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Input } from '$lib/components/ui/input/index.js';
	import { Label } from '$lib/components/ui/label/index.js';
	import * as Card from '$lib/components/ui/card/index.js';
	import * as Alert from '$lib/components/ui/alert/index.js';
	import * as Table from '$lib/components/ui/table/index.js';
	import { toast } from 'svelte-sonner';
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import {
		Alert02Icon,
		Clock01Icon,
		Database01Icon,
		InformationCircleIcon,
		RefreshIcon,
		RestoreBinIcon,
		Search01Icon
	} from '@hugeicons/core-free-icons';
	import { cn } from '$lib/utils.js';

	const id = $derived($page.params.id);

	let cluster = $state<Cluster | null>(null);
	let wal = $state<WalStreamStatus | null>(null);
	let segments = $state<WalSegment[]>([]);
	let loading = $state(false);
	let walNotEnabled = $state(false);
	let error = $state('');

	let targetTime = $state('');
	let pitrOpen = $state(false);
	let pitrSubmitting = $state(false);

	const targetIso = $derived(toIsoFromLocal(targetTime));
	const targetDate = $derived(targetIso ? new Date(targetIso) : null);

	const highlightedIds = $derived.by(() => {
		if (!targetDate || segments.length === 0) return [] as string[];
		const withTime = segments.filter((s) => s.archived_at);
		if (withTime.length === 0) return [] as string[];
		const windowMs = 30 * 60 * 1000;
		const near: string[] = [];
		for (const seg of withTime) {
			const t = new Date(seg.archived_at!).getTime();
			if (Math.abs(t - targetDate.getTime()) <= windowMs) {
				near.push(seg.id);
			}
		}
		if (near.length === 0) {
			let closest = withTime[0];
			let minDist = Math.abs(new Date(closest.archived_at!).getTime() - targetDate.getTime());
			for (const seg of withTime) {
				const dist = Math.abs(new Date(seg.archived_at!).getTime() - targetDate.getTime());
				if (dist < minDist) {
					minDist = dist;
					closest = seg;
				}
			}
			near.push(closest.id);
		}
		return near;
	});

	const pitrReady = $derived(
		Boolean(targetIso && cluster && wal?.enabled && !walNotEnabled)
	);

	function toIsoFromLocal(local: string): string | undefined {
		if (!local.trim()) return undefined;
		const d = new Date(local);
		if (Number.isNaN(d.getTime())) return undefined;
		return d.toISOString();
	}

	function fmtSize(n: number | null | undefined) {
		if (n == null) return '—';
		if (n < 1024) return `${n} B`;
		if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
		if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(2)} MB`;
		return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
	}

	function navigateToCluster(clusterId: string) {
		if (clusterId && clusterId !== id) {
			goto(`/clusters/${clusterId}/timetravel`);
		}
	}

	async function loadWalStatus() {
		const raw = await api<unknown>(`/api/clusters/${id}/wal`);
		wal = normalizeWalStatus(raw);
	}

	async function loadSegments() {
		const res = await api<WalSegmentsResponse | WalSegment[]>(
			`/api/clusters/${id}/wal/segments?limit=50`
		);
		if (Array.isArray(res)) {
			segments = res;
		} else {
			segments = res.segments ?? [];
		}
	}

	async function load() {
		loading = true;
		error = '';
		walNotEnabled = false;
		wal = null;
		segments = [];
		try {
			cluster = await api<Cluster>(`/api/clusters/${id}`);
			try {
				await loadWalStatus();
				if (wal?.searchable || wal?.enabled) {
					await loadSegments();
				}
			} catch (e) {
				const status = (e as { status?: number }).status;
				if (status === 404) {
					walNotEnabled = true;
				} else {
					throw e;
				}
			}
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to load time travel data';
		} finally {
			loading = false;
		}
	}

	async function confirmPitr() {
		if (!cluster || !targetIso) return;
		pitrSubmitting = true;
		try {
			const res = await api<{ operation_id: string }>(`/api/clusters/${id}/wal/pitr`, {
				method: 'POST',
				body: JSON.stringify({
					target_time: targetIso,
					confirm_cluster_name: cluster.name
				})
			});
			trackOperation(res.operation_id, {
				title: 'PITR recovery package',
				onDone: () => {
					toast.success(
						'Recovery package ready — see backup history for the manifest (base backup + WAL list + recovery.conf).'
					);
					load();
				}
			});
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'PITR package failed');
			throw e;
		} finally {
			pitrSubmitting = false;
		}
	}

	$effect(() => {
		if (id) load();
	});
</script>

<PageHeader
	title="Time travel"
	description="Browse a cluster's data as of a point in time using WAL segments and base backups. Pick a restore point, review WAL coverage, then prepare a recovery package or open the SQL browser."
/>

{#if error}
	<Alert.Root variant="destructive" class="mb-4">
		<HugeiconsIcon icon={Alert02Icon} class="size-4" strokeWidth={2} />
		<Alert.Description>{error}</Alert.Description>
	</Alert.Root>
{/if}

{#if walNotEnabled}
	<Alert.Root class="mb-4 border-border/60">
		<HugeiconsIcon icon={InformationCircleIcon} class="size-4" strokeWidth={2} />
		<Alert.Description>
			WAL archiving is not enabled for this cluster. Enable WAL archiving on
			<a href="/clusters/{id}/backup?tab=wal" class="font-medium underline underline-offset-2">
				Backups → WAL
			</a>
			first, then return here to pick a restore point.
		</Alert.Description>
	</Alert.Root>
{/if}

<Card.Root class="page-card mb-4 border-border/60">
	<Card.Header>
		<Card.Title class="flex items-center gap-2">
			<HugeiconsIcon icon={Clock01Icon} class="size-4" strokeWidth={2} />
			Pick restore point
		</Card.Title>
		<Card.Description>
			Choose the cluster and target timestamp. Coverage reflects archived WAL segments synced to the
			panel.
		</Card.Description>
	</Card.Header>
	<Card.Content class="space-y-4">
		<div class="grid gap-4 sm:grid-cols-2">
			<div class="space-y-2">
				<Label>Cluster</Label>
				<ClusterSelect value={id} onValueChange={navigateToCluster} />
				<p class="text-xs text-muted-foreground">Switch cluster above in the page header, or here.</p>
			</div>
			<div class="space-y-2">
				<Label for="target-time">Target time</Label>
				<Input id="target-time" type="datetime-local" bind:value={targetTime} />
			</div>
		</div>

		{#if loading && !wal}
			<p class="text-sm text-muted-foreground">Loading WAL status…</p>
		{:else if wal}
			<div class="flex flex-wrap items-center gap-3 rounded-lg border border-border/60 bg-muted/30 px-4 py-3 text-sm">
				<span class="text-muted-foreground">Coverage</span>
				<span class="tabular-nums">
					<strong>{wal.segment_count}</strong> segment{wal.segment_count === 1 ? '' : 's'}
				</span>
				<span class="text-muted-foreground">·</span>
				<span>
					Last synced:
					<strong>{wal.last_synced_at ? formatRelative(wal.last_synced_at) : '—'}</strong>
				</span>
				<span class="text-muted-foreground">·</span>
				<span>
					Searchable:
					<strong>{wal.searchable ? 'Yes' : 'No'}</strong>
				</span>
				<StatusBadge status={wal.status} />
			</div>
		{/if}

		<div class="flex justify-end">
			<Button variant="outline" size="sm" onclick={() => load()} disabled={loading}>
				<HugeiconsIcon icon={RefreshIcon} class="size-4" strokeWidth={2} />
				Refresh
			</Button>
		</div>
	</Card.Content>
</Card.Root>

<Card.Root class="page-card mb-4 border-border/60">
	<Card.Header>
		<Card.Title class="flex items-center gap-2">
			<HugeiconsIcon icon={Search01Icon} class="size-4" strokeWidth={2} />
			Available WAL window
		</Card.Title>
		<Card.Description>
			Recent archived segments (last 50). Rows near your target time are highlighted.
		</Card.Description>
	</Card.Header>
	<Card.Content>
		{#if walNotEnabled}
			<p class="text-sm text-muted-foreground">Enable WAL archiving to see segments.</p>
		{:else if loading && segments.length === 0}
			<p class="text-sm text-muted-foreground">Loading segments…</p>
		{:else if segments.length === 0}
			<p class="text-sm text-muted-foreground">No WAL segments archived yet.</p>
		{:else}
			<Table.Root>
				<Table.Header>
					<Table.Row class="hover:bg-transparent">
						<Table.Head>Filename</Table.Head>
						<Table.Head>Size</Table.Head>
						<Table.Head>Archived at</Table.Head>
					</Table.Row>
				</Table.Header>
				<Table.Body>
					{#each segments as seg (seg.id)}
						<Table.Row
							class={cn(
								highlightedIds.includes(seg.id) &&
									'bg-primary/10 ring-1 ring-inset ring-primary/30'
							)}
						>
							<Table.Cell class="font-mono text-xs">{seg.filename}</Table.Cell>
							<Table.Cell class="tabular-nums text-xs">{fmtSize(seg.size_bytes)}</Table.Cell>
							<Table.Cell class="text-xs whitespace-nowrap">
								{seg.archived_at ? formatRelative(seg.archived_at) : '—'}
							</Table.Cell>
						</Table.Row>
					{/each}
				</Table.Body>
			</Table.Root>
		{/if}
	</Card.Content>
</Card.Root>

<Card.Root class="page-card border-border/60">
	<Card.Header>
		<Card.Title class="flex items-center gap-2">
			<HugeiconsIcon icon={RestoreBinIcon} class="size-4" strokeWidth={2} />
			Actions
		</Card.Title>
		<Card.Description>
			Prepare a point-in-time recovery package or browse current data while live preview is in
			development.
		</Card.Description>
	</Card.Header>
	<Card.Content class="space-y-4">
		<Alert.Root class="border-border/60">
			<HugeiconsIcon icon={InformationCircleIcon} class="size-4" strokeWidth={2} />
			<Alert.Description>
				Live preview spins up a temporary restore in a future release. For now, prepare a PITR
				package or use <a
					href="/clusters/{id}/backup?tab=wal"
					class="font-medium underline underline-offset-2">Backups → WAL</a
				>.
			</Alert.Description>
		</Alert.Root>

		<div class="flex flex-wrap gap-3">
			<Button
				onclick={() => (pitrOpen = true)}
				disabled={!pitrReady || pitrSubmitting}
			>
				<HugeiconsIcon icon={RestoreBinIcon} class="size-4" strokeWidth={2} />
				Prepare PITR package
			</Button>
			<Button variant="outline" href="/clusters/{id}/browser">
				<HugeiconsIcon icon={Database01Icon} class="size-4" strokeWidth={2} />
				Open SQL browser (current)
			</Button>
		</div>
		<p class="text-xs text-muted-foreground">
			The SQL browser shows live data only. Historical browsing at the selected restore point will be
			available when live preview ships.
		</p>
	</Card.Content>
</Card.Root>

<ConfirmDialog
	bind:open={pitrOpen}
	title="Prepare PITR recovery package?"
	description="This builds a recovery manifest (base backup + WAL segment list + recovery.conf) for the selected timestamp. It does not modify or replace the running cluster."
	confirmLabel="Prepare package"
	loading={pitrSubmitting}
	onConfirm={confirmPitr}
/>
