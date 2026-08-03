<script lang="ts">
	import { api, type MonitoringOverview } from '$lib/api';
	import EmptyState from '$lib/components/empty-state.svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import * as Card from '$lib/components/ui/card/index.js';
	import * as Table from '$lib/components/ui/table/index.js';
	import * as Alert from '$lib/components/ui/alert/index.js';
	import * as Tabs from '$lib/components/ui/tabs/index.js';
	import { Skeleton } from '$lib/components/ui/skeleton/index.js';
	import { Badge } from '$lib/components/ui/badge/index.js';
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import { Alert02Icon, Chart01Icon, RefreshIcon } from '@hugeicons/core-free-icons';

	const CHART_WIDTH = 640;
	const CHART_HEIGHT = 200;
	const PADDING = 8;

	let overview = $state<MonitoringOverview | null>(null);
	let error = $state('');
	let loading = $state(true);
	let hours = $state('24');

	$effect(() => {
		if (hours) load();
	});

	async function load() {
		loading = true;
		error = '';
		try {
			overview = await api<MonitoringOverview>(`/api/monitoring/overview?hours=${hours}`);
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to load monitoring';
		} finally {
			loading = false;
		}
	}

	const rangeLabel = $derived(hours === '168' ? '7 days' : '24 hours');
	const series = $derived(overview?.series ?? []);

	const maxCpu = $derived(series.length ? Math.max(...series.map((p) => p.cpu), 1) : 1);
	const maxMem = $derived(series.length ? Math.max(...series.map((p) => p.memory_mb), 1) : 1);

	function polylinePoints(values: number[], max: number): string {
		if (values.length === 0) return '';
		const innerW = CHART_WIDTH - PADDING * 2;
		const innerH = CHART_HEIGHT - PADDING * 2;
		const stepX = values.length > 1 ? innerW / (values.length - 1) : 0;
		return values
			.map((v, i) => {
				const x = PADDING + i * stepX;
				const y = PADDING + innerH - (v / max) * innerH;
				return `${x},${y}`;
			})
			.join(' ');
	}

	const cpuPoints = $derived(polylinePoints(series.map((p) => p.cpu), maxCpu));
	const memPoints = $derived(polylinePoints(series.map((p) => p.memory_mb), maxMem));

	function formatTick(at: string): string {
		if (at.length >= 13) return at.slice(11, 13) + ':00';
		return at;
	}

	const xLabels = $derived.by(() => {
		if (series.length === 0) return [] as { key: string; label: string }[];
		if (series.length <= 12) {
			return series.map((p, i) => ({ key: `${i}-${p.at}`, label: formatTick(p.at) }));
		}
		const idxs = [0, Math.floor(series.length / 2), series.length - 1];
		const seen = new Set<number>();
		const out: { key: string; label: string }[] = [];
		for (const i of idxs) {
			if (seen.has(i)) continue;
			seen.add(i);
			out.push({ key: `${i}-${series[i].at}`, label: formatTick(series[i].at) });
		}
		return out;
	});
</script>

<div class="mb-6 flex flex-wrap items-center justify-between gap-3">
	<Badge variant="outline" class="text-xs">Range: {rangeLabel}</Badge>
	<div class="flex flex-wrap items-center gap-2">
		<Tabs.Root bind:value={hours}>
			<Tabs.List class="h-9">
				<Tabs.Trigger value="24" class="px-3 text-xs">24h</Tabs.Trigger>
				<Tabs.Trigger value="168" class="px-3 text-xs">7d</Tabs.Trigger>
			</Tabs.List>
		</Tabs.Root>
		<Button variant="outline" size="sm" onclick={() => load()} disabled={loading}>
			<HugeiconsIcon icon={RefreshIcon} class="size-4" strokeWidth={2} />
			Refresh
		</Button>
	</div>
</div>

{#if error}
	<Alert.Root variant="destructive" class="mb-4">
		<HugeiconsIcon icon={Alert02Icon} class="size-4" strokeWidth={2} />
		<Alert.Description>{error}</Alert.Description>
	</Alert.Root>
{/if}

{#if loading && !overview}
	<Skeleton class="mb-6 h-72 rounded-xl" />
	<Skeleton class="h-72 rounded-xl" />
{:else if overview}
	{#if series.length === 0}
		<EmptyState
			title="No trend data yet"
			description="Hourly fleet averages appear after cluster metrics are collected. Sparse schedules may leave long gaps."
		/>
	{:else}
		<div class="grid gap-6 xl:grid-cols-2">
			<Card.Root class="page-card">
				<Card.Header>
					<Card.Title class="flex items-center gap-2">
						<HugeiconsIcon icon={Chart01Icon} class="size-4" strokeWidth={2} />
						Fleet CPU %
					</Card.Title>
					<Card.Description>Average CPU across clusters — last {rangeLabel}</Card.Description>
				</Card.Header>
				<Card.Content>
					<div class="mb-2 flex justify-between text-xs text-muted-foreground tabular-nums">
						<span>max {maxCpu.toFixed(1)}%</span>
						<span>0%</span>
					</div>
					<svg
						viewBox="0 0 {CHART_WIDTH} {CHART_HEIGHT}"
						class="w-full rounded-lg bg-muted/30"
						role="img"
						aria-label="Fleet CPU trend"
					>
						<line
							x1={PADDING}
							y1={CHART_HEIGHT - PADDING}
							x2={CHART_WIDTH - PADDING}
							y2={CHART_HEIGHT - PADDING}
							class="stroke-border"
							stroke-width="1"
						/>
						{#if cpuPoints}
							<polyline
								points={cpuPoints}
								fill="none"
								class="stroke-primary"
								stroke-width="2"
								stroke-linejoin="round"
								stroke-linecap="round"
							/>
						{/if}
					</svg>
					<div class="mt-2 flex justify-between text-xs text-muted-foreground">
						{#each xLabels as tick (tick.key)}
							<span>{tick.label}</span>
						{/each}
					</div>
				</Card.Content>
			</Card.Root>

			<Card.Root class="page-card">
				<Card.Header>
					<Card.Title class="flex items-center gap-2">
						<HugeiconsIcon icon={Chart01Icon} class="size-4" strokeWidth={2} />
						Fleet memory MB
					</Card.Title>
					<Card.Description>Average memory usage — last {rangeLabel}</Card.Description>
				</Card.Header>
				<Card.Content>
					<div class="mb-2 flex justify-between text-xs text-muted-foreground tabular-nums">
						<span>max {Math.round(maxMem)} MB</span>
						<span>0 MB</span>
					</div>
					<svg
						viewBox="0 0 {CHART_WIDTH} {CHART_HEIGHT}"
						class="w-full rounded-lg bg-muted/30"
						role="img"
						aria-label="Fleet memory trend"
					>
						<line
							x1={PADDING}
							y1={CHART_HEIGHT - PADDING}
							x2={CHART_WIDTH - PADDING}
							y2={CHART_HEIGHT - PADDING}
							class="stroke-border"
							stroke-width="1"
						/>
						{#if memPoints}
							<polyline
								points={memPoints}
								fill="none"
								class="stroke-sky-500"
								stroke-width="2"
								stroke-linejoin="round"
								stroke-linecap="round"
							/>
						{/if}
					</svg>
					<div class="mt-2 flex justify-between text-xs text-muted-foreground">
						{#each xLabels as tick (tick.key)}
							<span>{tick.label}</span>
						{/each}
					</div>
				</Card.Content>
			</Card.Root>
		</div>

		<Card.Root class="page-card mt-6">
			<Card.Header>
				<Card.Title>Sample data</Card.Title>
				<Card.Description>Hourly fleet averages (most recent rows)</Card.Description>
			</Card.Header>
			<Card.Content class="p-0">
				<div class="table-scroll max-h-80">
					<Table.Root>
						<Table.Header>
							<Table.Row class="hover:bg-transparent">
								<Table.Head>Time</Table.Head>
								<Table.Head class="text-right">CPU %</Table.Head>
								<Table.Head class="text-right">Memory MB</Table.Head>
							</Table.Row>
						</Table.Header>
						<Table.Body>
							{#each series.slice(-24) as point, i (`${i}-${point.at}`)}
								<Table.Row>
									<Table.Cell class="font-mono text-xs">{point.at}</Table.Cell>
									<Table.Cell class="text-right tabular-nums text-xs">
										{point.cpu.toFixed(1)}
									</Table.Cell>
									<Table.Cell class="text-right tabular-nums text-xs">
										{Math.round(point.memory_mb)}
									</Table.Cell>
								</Table.Row>
							{/each}
						</Table.Body>
					</Table.Root>
				</div>
			</Card.Content>
		</Card.Root>
	{/if}
{/if}
