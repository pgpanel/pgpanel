<script lang="ts">
  import { page } from '$app/stores';
  import { api } from '$lib/api';

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

<div class="mb-4">
  <a class="text-sm text-slate-400 hover:text-white" href={`/clusters/${id}`}>← Cluster</a>
  <h1 class="mt-2 text-2xl font-semibold">SQL console</h1>
  <p class="text-sm text-slate-400">Read-only by default (SELECT / EXPLAIN / SHOW / WITH…SELECT). Parser + READ ONLY transaction.</p>
</div>

<div class="card mb-4 space-y-3">
  <div class="flex flex-wrap gap-3">
    <div>
      <label class="label" for="db">Database</label>
      <input id="db" class="input w-48" bind:value={database} />
    </div>
  </div>
  <textarea class="input min-h-40 font-mono text-sm" bind:value={sql}></textarea>
  <button class="btn-primary" disabled={loading} onclick={run}>{loading ? 'Running…' : 'Run query'}</button>
</div>

{#if error}<div class="mb-4 text-sm text-rose-300">{error}</div>{/if}

{#if result}
  <div class="card overflow-x-auto">
    <div class="mb-2 text-sm text-slate-400">{result.row_count} rows · {result.duration_ms} ms{#if result.truncated} · truncated{/if}</div>
    <table class="w-full text-sm">
      <thead class="text-xs text-slate-500">
        <tr>{#each result.columns as c}<th class="px-2 py-1 text-left">{c}</th>{/each}</tr>
      </thead>
      <tbody>
        {#each result.rows as row}
          <tr class="border-t border-slate-800">
            {#each row as cell}
              <td class="max-w-xs truncate px-2 py-1 font-mono text-xs">{JSON.stringify(cell)}</td>
            {/each}
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{/if}
