<script lang="ts">
  import { page } from '$app/stores';
  import { api, type TableInfo } from '$lib/api';

  interface SchemaInfo { name: string }
  interface TableDetails {
    columns: { name: string; data_type: string; is_nullable: boolean; is_primary_key: boolean }[];
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

<div class="mb-4">
  <a class="text-sm text-slate-400 hover:text-white" href={`/clusters/${id}`}>← Cluster</a>
  <h1 class="mt-2 text-2xl font-semibold">Database browser</h1>
  <p class="text-sm text-slate-400">Read-only metadata and rows</p>
</div>

{#if error}<p class="mb-3 text-sm text-rose-300">{error}</p>{/if}

<div class="mb-4 flex flex-wrap gap-3">
  <div>
    <label class="label" for="db">Database</label>
    <input id="db" class="input w-48" bind:value={database} onchange={() => init()} />
  </div>
  <div>
    <label class="label" for="schema">Schema</label>
    <select id="schema" class="input w-48" bind:value={schema} onchange={() => loadTables()}>
      {#each schemas as s}
        <option value={s.name}>{s.name}</option>
      {/each}
    </select>
  </div>
</div>

<div class="grid gap-4 lg:grid-cols-[240px_1fr]">
  <div class="card max-h-[70vh] overflow-auto">
    <h2 class="mb-2 text-sm font-medium text-slate-400">Tables</h2>
    {#each tables as t}
      <button
        class="mb-1 block w-full rounded px-2 py-1 text-left text-sm hover:bg-slate-800 {table === t.name ? 'bg-slate-800 text-emerald-300' : ''}"
        onclick={() => openTable(t.name)}
      >
        {t.name}
      </button>
    {:else}
      <p class="text-sm text-slate-500">No tables</p>
    {/each}
  </div>

  <div class="space-y-4">
    {#if details}
      <div class="card overflow-x-auto">
        <h2 class="mb-2 font-medium">{schema}.{table}</h2>
        <table class="w-full text-sm">
          <thead class="text-xs text-slate-500"><tr><th class="py-1 text-left">Column</th><th class="text-left">Type</th><th class="text-left">Null</th><th class="text-left">PK</th></tr></thead>
          <tbody>
            {#each details.columns as c}
              <tr class="border-t border-slate-800">
                <td class="py-1 font-mono">{c.name}</td>
                <td>{c.data_type}</td>
                <td>{c.is_nullable ? 'yes' : 'no'}</td>
                <td>{c.is_primary_key ? '✓' : ''}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}

    {#if rows}
      <div class="card overflow-x-auto">
        <h2 class="mb-2 font-medium">Rows</h2>
        <table class="w-full text-sm">
          <thead class="text-xs text-slate-500">
            <tr>{#each rows.columns as c}<th class="px-2 py-1 text-left">{c}</th>{/each}</tr>
          </thead>
          <tbody>
            {#each rows.rows as row}
              <tr class="border-t border-slate-800">
                {#each row as cell}
                  <td class="max-w-xs truncate px-2 py-1 font-mono text-xs">{JSON.stringify(cell)}</td>
                {/each}
              </tr>
            {:else}
              <tr><td class="py-3 text-slate-500" colspan={rows.columns.length}>Empty</td></tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </div>
</div>
