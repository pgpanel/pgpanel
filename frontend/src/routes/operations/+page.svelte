<script lang="ts">
  import { onMount } from 'svelte';
  import { api, statusBadge, type Operation } from '$lib/api';

  let ops = $state<Operation[]>([]);
  let error = $state('');

  onMount(() => {
    const load = () =>
      api<Operation[]>('/api/operations')
        .then((o) => (ops = o))
        .catch((e) => (error = e.message));
    load();
    const t = setInterval(load, 3000);
    return () => clearInterval(t);
  });
</script>

<h1 class="mb-6 text-2xl font-semibold">Operations</h1>
{#if error}<p class="text-rose-300">{error}</p>{/if}

<div class="card overflow-x-auto">
  <table class="w-full text-sm">
    <thead class="text-xs uppercase text-slate-500">
      <tr>
        <th class="py-2 text-left">Type</th>
        <th class="text-left">Status</th>
        <th class="text-left">Progress</th>
        <th class="text-left">Error</th>
        <th class="text-left">Updated</th>
      </tr>
    </thead>
    <tbody>
      {#each ops as op}
        <tr class="border-t border-slate-800">
          <td class="py-2 font-mono text-xs">{op.job_type}<div class="text-slate-500">{op.id.slice(0, 8)}</div></td>
          <td><span class={statusBadge(op.status)}>{op.status}</span></td>
          <td>{op.progress}%</td>
          <td class="max-w-xs truncate text-rose-300">{op.error ?? ''}</td>
          <td class="text-xs text-slate-500">{op.updated_at}</td>
        </tr>
      {:else}
        <tr><td class="py-4 text-slate-500" colspan="5">No operations</td></tr>
      {/each}
    </tbody>
  </table>
</div>
