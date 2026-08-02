<script lang="ts">
	import { page } from '$app/stores';
	import { api, type TableInfo } from '$lib/api';
	import PageHeader from '$lib/components/page-header.svelte';
	import DatabaseSelect from '$lib/components/database-select.svelte';
	import { Label } from '$lib/components/ui/label/index.js';
	import * as Card from '$lib/components/ui/card/index.js';
	import * as Table from '$lib/components/ui/table/index.js';
	import * as Select from '$lib/components/ui/select/index.js';
	import * as Alert from '$lib/components/ui/alert/index.js';
	import { ScrollArea } from '$lib/components/ui/scroll-area/index.js';
	import { Badge } from '$lib/components/ui/badge/index.js';
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import { Alert02Icon } from '@hugeicons/core-free-icons';
	import { cn } from '$lib/utils.js';

	interface SchemaInfo {
		name: string;
	}
	interface TableDetails {
		columns: {
			name: string;
			data_type: string;
			is_nullable: boolean;
			is_primary_key: boolean;
		}[];
	}
	interface RowsResponse {
		columns: string[];
		rows: unknown[][];
		page: number;
		page_size: number;
	}

	const id = $derived($page.params.id);
	let database = $state('postgres');
	let schemas = $state<SchemaInfo[]>([]);
	let tables = $state<TableInfo[]>([]);
	let schema = $state('public');
	let table = $state('');
	let details = $state<TableDetails | null>(null);
	let rows = $state<RowsResponse | null>(null);
	let error = $state('');

	async function loadSchemas() {
		schemas = await api<SchemaInfo[]>(`/api/clusters/${id}/schemas?database=${database}`);
		if (schemas.length && !schemas.find((s) => s.name === schema)) {
			schema = schemas[0].name;
		}
	}

	async function loadTables() {
		tables = await api<TableInfo[]>(
			`/api/clusters/${id}/tables?database=${database}&schema=${schema}`
		);
	}

	async function openTable(t: string) {
		table = t;
		details = await api<TableDetails>(
			`/api/clusters/${id}/tables/${schema}/${t}?database=${database}`
		);
		rows = await api<RowsResponse>(
			`/api/clusters/${id}/tables/${schema}/${t}/rows?database=${database}&page=1&page_size=50`
		);
	}

	async function init() {
		try {
			error = '';
			await loadSchemas();
			await loadTables();
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed';
		}
	}

	$effect(() => {
		if (id) init();
	});
</script>

<PageHeader title="Database browser" description="Read-only metadata and rows · max 50 rows default" />

{#if error}
	<Alert.Root variant="destructive" class="mb-4">
		<HugeiconsIcon icon={Alert02Icon} class="size-4" strokeWidth={2} />
		<Alert.Description>{error}</Alert.Description>
	</Alert.Root>
{/if}

<div class="mb-4 flex flex-wrap gap-4">
	<div class="space-y-1.5">
		<Label for="db">Database</Label>
		<DatabaseSelect
			id="db"
			clusterId={id}
			class="w-48"
			bind:value={database}
			onValueChange={() => init()}
		/>
	</div>
	<div class="space-y-1.5">
		<Label>Schema</Label>
		<Select.Root
			type="single"
			value={schema}
			onValueChange={(v) => {
				if (v) {
					schema = v;
					loadTables();
				}
			}}
		>
			<Select.Trigger class="w-48 font-mono">{schema}</Select.Trigger>
			<Select.Content>
				{#each schemas as s}
					<Select.Item value={s.name} label={s.name}>{s.name}</Select.Item>
				{/each}
			</Select.Content>
		</Select.Root>
	</div>
</div>

<div class="grid gap-4 lg:grid-cols-[240px_1fr]">
	<Card.Root class="border-border/60 overflow-hidden">
		<Card.Header class="py-3">
			<Card.Title class="text-sm">Tables</Card.Title>
		</Card.Header>
		<ScrollArea class="h-[60vh] px-2 pb-3">
			{#each tables as t}
				<button
					type="button"
					class={cn(
						'mb-0.5 flex w-full items-center justify-between rounded-md px-2 py-1.5 text-left text-sm transition hover:bg-muted',
						table === t.name && 'bg-primary/15 text-primary font-medium'
					)}
					onclick={() => openTable(t.name)}
				>
					<span class="truncate font-mono text-xs">{t.name}</span>
					{#if t.table_type !== 'table'}
						<Badge variant="outline" class="text-[10px]">{t.table_type}</Badge>
					{/if}
				</button>
			{:else}
				<p class="px-2 py-4 text-sm text-muted-foreground">No tables</p>
			{/each}
		</ScrollArea>
	</Card.Root>

	<div class="space-y-4 min-w-0">
		{#if details}
			<Card.Root class="border-border/60 overflow-hidden">
				<Card.Header>
					<Card.Title class="font-mono text-base">{schema}.{table}</Card.Title>
					<Card.Description>Columns</Card.Description>
				</Card.Header>
				<Card.Content class="p-0">
					<Table.Root>
						<Table.Header>
							<Table.Row>
								<Table.Head>Column</Table.Head>
								<Table.Head>Type</Table.Head>
								<Table.Head>Null</Table.Head>
								<Table.Head>PK</Table.Head>
							</Table.Row>
						</Table.Header>
						<Table.Body>
							{#each details.columns as c}
								<Table.Row>
									<Table.Cell class="font-mono text-xs">{c.name}</Table.Cell>
									<Table.Cell class="text-muted-foreground text-xs">{c.data_type}</Table.Cell>
									<Table.Cell>{c.is_nullable ? 'yes' : 'no'}</Table.Cell>
									<Table.Cell>{c.is_primary_key ? '✓' : ''}</Table.Cell>
								</Table.Row>
							{/each}
						</Table.Body>
					</Table.Root>
				</Card.Content>
			</Card.Root>
		{/if}

		{#if rows}
			<Card.Root class="border-border/60 overflow-hidden">
				<Card.Header>
					<Card.Title class="text-base">Rows</Card.Title>
					<Card.Description>Page {rows.page} · size {rows.page_size}</Card.Description>
				</Card.Header>
				<Card.Content class="overflow-x-auto p-0">
					<Table.Root>
						<Table.Header>
							<Table.Row>
								{#each rows.columns as c}
									<Table.Head class="font-mono text-xs">{c}</Table.Head>
								{/each}
							</Table.Row>
						</Table.Header>
						<Table.Body>
							{#each rows.rows as row}
								<Table.Row>
									{#each row as cell}
										<Table.Cell class="max-w-[220px] truncate font-mono text-xs">
											{JSON.stringify(cell)}
										</Table.Cell>
									{/each}
								</Table.Row>
							{:else}
								<Table.Row>
									<Table.Cell colspan={rows.columns.length} class="text-muted-foreground">
										Empty
									</Table.Cell>
								</Table.Row>
							{/each}
						</Table.Body>
					</Table.Root>
				</Card.Content>
			</Card.Root>
		{:else if !details}
			<Card.Root class="border-border/60 border-dashed">
				<Card.Content class="py-16 text-center text-sm text-muted-foreground">
					Select a table to inspect schema and rows
				</Card.Content>
			</Card.Root>
		{/if}
	</div>
</div>
