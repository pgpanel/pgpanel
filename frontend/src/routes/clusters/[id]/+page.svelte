<script lang="ts">
  import { onMount } from 'svelte';
  import { page } from '$app/stores';
  import { api, statusBadge, type Cluster } from '$lib/api';

  let cluster = $state<Cluster | null>(null);
  let error = $state('');
  let busy = $state('');

  const id = $derived($page.params.id);

  async function load() {
    cluster = await api<Cluster>(`/api/clusters/${id}`);
  }

  onMount(() => {
    load().catch((e) => (error = e.message));
    const t = setInterval(() => load().catch(() => {}), 5000);
    return () => clearInterval(t);
  });

  async function action(path: string, label: string) {
    busy = label;
    error = '';
    try {
      await api(`/api/clusters/${id}/${path}`, { method: 'POST' });
      await load();
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed';
    } finally {
      busy = '';
    }
  }
</script>

{#if cluster}
  <div class="mb-6 flex flex-wrap items-start justify-between gap-4">
    <div>
      <h1 class="text-2xl font-semibold">{cluster.name}</h1>
      <div class="mt-1 flex flex-wrap items-center gap-2 text-sm text-slate-400">
        <span class={statusBadge(cluster.status)}>{cluster.status}</span>
        <span>PG {cluster.postgres_version}</span>
        <span>{cluster.slug}</span>
      </div>
    </div>
    <div class="flex flex-wrap gap-2">
      <button class="btn-secondary" disabled={!!busy} onclick={() => action('start', 'start')}>Start</button>
      <button class="btn-secondary" disabled={!!busy} onclick={() => action('stop', 'stop')}>Stop</button>
      <button class="btn-secondary" disabled={!!busy} onclick={() => action('restart', 'restart')}>Restart</button>
    </div>
  </div>

  {#if error}<p class="mb-4 text-sm text-rose-300">{error}</p>{/if}

  <div class="mb-6 flex flex-wrap gap-2 text-sm">
    <a class="btn-secondary" href={`/clusters/${id}/databases`}>Databases</a>
    <a class="btn-secondary" href={`/clusters/${id}/browser`}>Browser</a>
    <a class="btn-secondary" href={`/clusters/${id}/query`}>SQL console</a>
    <a class="btn-secondary" href={`/clusters/${id}/backup`}>Backup</a>
  </div>

  <div class="grid gap-4 md:grid-cols-2">
    <div class="card space-y-2 text-sm">
      <div class="flex justify-between"><span class="text-slate-400">Internal host</span><span class="font-mono">{cluster.internal_hostname ?? '—'}</span></div>
      <div class="flex justify-between"><span class="text-slate-400">Public port</span><span>{cluster.public_port ?? 'not exposed'}</span></div>
      <div class="flex justify-between"><span class="text-slate-400">CPU / Memory</span><span>{cluster.cpu_limit} / {cluster.memory_mb} MB</span></div>
      <div class="flex justify-between"><span class="text-slate-400">Storage limit</span><span>{cluster.storage_limit_gb} GB</span></div>
      <div class="flex justify-between"><span class="text-slate-400">Databasus</span><span>{cluster.databasus_status}</span></div>
      <div class="flex justify-between"><span class="text-slate-400">Delete protection</span><span>{cluster.delete_protection ? 'on' : 'off'}</span></div>
    </div>
    {#if cluster.last_error}
      <div class="card border-rose-900/50 text-sm text-rose-200">
        <div class="mb-1 font-medium">Last error</div>
        {cluster.last_error}
      </div>
    {/if}
  </div>
{:else}
  <p class="text-slate-400">Loading cluster…</p>
{/if}
