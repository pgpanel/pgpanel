<script lang="ts">
	import { page } from '$app/stores';
	import { api } from '$lib/api';
	import PageHeader from '$lib/components/page-header.svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Input } from '$lib/components/ui/input/index.js';
	import { Label } from '$lib/components/ui/label/index.js';
	import { Textarea } from '$lib/components/ui/textarea/index.js';
	import * as Card from '$lib/components/ui/card/index.js';
	import * as Table from '$lib/components/ui/table/index.js';
	import * as Alert from '$lib/components/ui/alert/index.js';
	import { Badge } from '$lib/components/ui/badge/index.js';
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import { ArrowLeft01Icon, Alert02Icon, PlayIcon } from '@hugeicons/core-free-icons';

	interface SqlResult {
		columns: string[];
		rows: unknown[][];
		row_count: number;
		truncated: boolean;
		duration_ms: number;
	}

	const id = $derived($page.params.id);
	let database = $state('postgres');
	let sql = $state('SELECT version();');
	let result = $state<SqlResult | null>(null);
	let error = $state('');
	let loading = $state(false);

	async function run() {
		loading = true;
		error = '';
		result = null;
		try {
			result = await api<SqlResult>(`/api/clusters/${id}/query`, {
				method: 'POST',
				body: JSON.stringify({ database, sql, admin_mode: false })
			});
		} catch (e) {
			error = e instanceof Error ? e.message : 'Query failed';
		} finally {
			loading = false;
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
	title="SQL console"
	description="Read-only: SELECT / EXPLAIN / SHOW / WITH…SELECT · parser + READ ONLY transaction"
/>

<Card.Root class="mb-4 border-border/60">
	<Card.Content class="space-y-4 pt-6">
		<div class="flex flex-wrap items-end gap-4">
			<div class="space-y-1.5">
				<Label for="db">Database</Label>
				<Input id="db" class="w-48 font-mono" bind:value={database} />
			</div>
			<Button onclick={run} disabled={loading}>
				<HugeiconsIcon icon={PlayIcon} class="size-4" strokeWidth={2} />
				{loading ? 'Running…' : 'Run query'}
			</Button>
		</div>
		<div class="space-y-1.5">
			<Label for="sql">SQL</Label>
			<Textarea
				id="sql"
				class="min-h-40 font-mono text-sm"
				bind:value={sql}
				spellcheck={false}
			/>
		</div>
	</Card.Content>
</Card.Root>

{#if error}
	<Alert.Root variant="destructive" class="mb-4">
		<HugeiconsIcon icon={Alert02Icon} class="size-4" strokeWidth={2} />
		<Alert.Description>{error}</Alert.Description>
	</Alert.Root>
{/if}

{#if result}
	<Card.Root class="border-border/60 overflow-hidden">
		<Card.Header class="flex-row flex-wrap items-center gap-2 space-y-0">
			<Card.Title class="text-base">Results</Card.Title>
			<Badge variant="secondary">{result.row_count} rows</Badge>
			<Badge variant="outline">{result.duration_ms} ms</Badge>
			{#if result.truncated}
				<Badge variant="destructive">truncated</Badge>
			{/if}
		</Card.Header>
		<Card.Content class="overflow-x-auto p-0">
			<Table.Root>
				<Table.Header>
					<Table.Row>
						{#each result.columns as c}
							<Table.Head class="font-mono text-xs">{c}</Table.Head>
						{/each}
					</Table.Row>
				</Table.Header>
				<Table.Body>
					{#each result.rows as row}
						<Table.Row>
							{#each row as cell}
								<Table.Cell class="max-w-[240px] truncate font-mono text-xs">
									{JSON.stringify(cell)}
								</Table.Cell>
							{/each}
						</Table.Row>
					{/each}
				</Table.Body>
			</Table.Root>
		</Card.Content>
	</Card.Root>
{/if}
