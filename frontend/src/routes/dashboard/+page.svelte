<script lang="ts">
  import { onMount } from 'svelte';
  import { api, statusBadge, type Cluster, type DashboardStats, type Operation } from '$lib/api';

  let stats = $state<DashboardStats | null>(null);
  let clusters = $state<Cluster[]>([]);
  let ops = $state<Operation[]>([]);
  let error = $state('');

  onMount(async () => {
    try {
      stats = await api<DashboardStats>('/api/dashboard');
      clusters = await api<Cluster[]>('/api/clusters');
      ops = await api<Operation[]>('/api/operations');
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed to load';
    }
  });
</script>

<div class="mb-6 flex items-end justify-between gap-4">
  <div>
    <h1 class="text-2xl font-semibold tracking-tight">Dashboard</h1>
    <p class="text-sm text-slate-400">Cluster health, backups, and active operations</p>
  </div>
  <a class="btn-primary" href="/clusters/new">New cluster</a>
</div>

{#if error}
  <div class="mb-4 rounded-lg border border-rose-800 bg-rose-950/40 p-3 text-sm text-rose-200">{error}</div>
{/if}

{#if stats}
  <div class="mb-8 grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
    <div class="card"><div class="text-xs text-slate-400">Clusters</div><div class="mt-1 text-3xl font-semibold">{stats.cluster_count}</div></div>
    <div class="card"><div class="text-xs text-slate-400">Healthy</div><div class="mt-1 text-3xl font-semibold text-emerald-400">{stats.healthy_count}</div></div>
    <div class="card"><div class="text-xs text-slate-400">Degraded / warning</div><div class="mt-1 text-3xl font-semibold text-amber-400">{stats.degraded_count}</div></div>
    <div class="card"><div class="text-xs text-slate-400">Databases</div><div class="mt-1 text-3xl font-semibold">{stats.database_count}</div></div>
    <div class="card"><div class="text-xs text-slate-400">Active operations</div><div class="mt-1 text-3xl font-semibold">{stats.active_operations}</div></div>
    <div class="card"><div class="text-xs text-slate-400">Failed operations</div><div class="mt-1 text-3xl font-semibold text-rose-400">{stats.failed_operations}</div></div>
  </div>
{/if}

<div class="grid gap-6 xl:grid-cols-2">
  <section class="card">
    <h2 class="mb-4 font-medium">Clusters</h2>
    <div class="space-y-2">
      {#each clusters as c}
        <a class="flex items-center justify-between rounded-lg border border-slate-800 px-3 py-2 hover:bg-slate-800/50" href={`/clusters/${c.id}`}>
          <div>
            <div class="font-medium">{c.name}</div>
            <div class="text-xs text-slate-500">PostgreSQL {c.postgres_version} · {c.slug}</div>
          </div>
          <span class={statusBadge(c.status)}>{c.status}</span>
        </a>
      {:else}
        <p class="text-sm text-slate-500">No clusters yet.</p>
      {/each}
    </div>
  </section>

  <section class="card">
    <h2 class="mb-4 font-medium">Recent operations</h2>
    <div class="space-y-2">
      {#each ops.slice(0, 8) as op}
        <a class="flex items-center justify-between rounded-lg border border-slate-800 px-3 py-2 hover:bg-slate-800/50" href="/operations">
          <div>
            <div class="font-medium">{op.job_type}</div>
            <div class="text-xs text-slate-500">{op.id.slice(0, 8)}… · {op.progress}%</div>
          </div>
          <span class={statusBadge(op.status)}>{op.status}</span>
        </a>
      {:else}
        <p class="text-sm text-slate-500">No operations yet.</p>
      {/each}
    </div>
  </section>
</div>
