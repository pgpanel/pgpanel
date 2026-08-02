<script lang="ts">
  import { onMount } from 'svelte';
  import { api, statusBadge, type Cluster } from '$lib/api';

  let clusters = $state<Cluster[]>([]);
  let error = $state('');

  onMount(async () => {
    try {
      clusters = await api<Cluster[]>('/api/clusters');
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed';
    }
  });
</script>

<div class="mb-6 flex items-center justify-between">
  <h1 class="text-2xl font-semibold">Clusters</h1>
  <a class="btn-primary" href="/clusters/new">New cluster</a>
</div>

{#if error}<p class="text-rose-300">{error}</p>{/if}

<div class="card overflow-x-auto">
  <table class="w-full text-left text-sm">
    <thead class="text-xs uppercase text-slate-500">
      <tr>
        <th class="px-2 py-2">Name</th>
        <th class="px-2 py-2">Version</th>
        <th class="px-2 py-2">Status</th>
        <th class="px-2 py-2">CPU / RAM</th>
        <th class="px-2 py-2">Backup</th>
      </tr>
    </thead>
    <tbody>
      {#each clusters as c}
        <tr class="border-t border-slate-800 hover:bg-slate-800/40">
          <td class="px-2 py-3">
            <a class="font-medium text-emerald-400 hover:underline" href={`/clusters/${c.id}`}>{c.name}</a>
            <div class="text-xs text-slate-500">{c.slug}</div>
          </td>
          <td class="px-2 py-3">{c.postgres_version}</td>
          <td class="px-2 py-3"><span class={statusBadge(c.status)}>{c.status}</span></td>
          <td class="px-2 py-3">{c.cpu_limit} CPU · {c.memory_mb} MB</td>
          <td class="px-2 py-3">{c.databasus_status}</td>
        </tr>
      {:else}
        <tr><td class="px-2 py-6 text-slate-500" colspan="5">No clusters. Create one to get started.</td></tr>
      {/each}
    </tbody>
  </table>
</div>
