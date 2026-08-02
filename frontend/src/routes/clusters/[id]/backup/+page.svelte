<script lang="ts">
  import { onMount } from 'svelte';
  import { page } from '$app/stores';
  import { api } from '$lib/api';

  interface BackupStatus {
    integration_status: string;
    last_successful_backup: string | null;
    last_backup_status: string | null;
    backup_lag_seconds: number | null;
    wal_status: string | null;
    failed_backups: number;
    message: string | null;
    manual_setup_info: unknown;
  }

  const id = $derived($page.params.id);
  let status = $state<BackupStatus | null>(null);
  let error = $state('');
  let message = $state('');

  async function load() {
    status = await api<BackupStatus>(`/api/clusters/${id}/backup`);
  }

  onMount(() => {
    load().catch((e) => (error = e.message));
  });

  async function register() {
    error = '';
    try {
      const res = await api<{ operation_id: string }>(`/api/clusters/${id}/backup/register`, {
        method: 'POST'
      });
      message = `Registration queued: ${res.operation_id}`;
      await load();
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed';
    }
  }

  async function trigger() {
    error = '';
    try {
      await api(`/api/clusters/${id}/backup/trigger`, { method: 'POST' });
      message = 'Backup triggered';
      await load();
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed';
    }
  }
</script>

<div class="mb-4">
  <a class="text-sm text-slate-400 hover:text-white" href={`/clusters/${id}`}>← Cluster</a>
  <h1 class="mt-2 text-2xl font-semibold">Backup (Databasus)</h1>
  <p class="text-sm text-slate-400">Backups, WAL, and PITR are handled by Databasus — not reimplemented in PgPanel.</p>
</div>

{#if error}<p class="mb-3 text-sm text-rose-300">{error}</p>{/if}
{#if message}<p class="mb-3 text-sm text-emerald-300">{message}</p>{/if}

{#if status}
  <div class="card mb-4 space-y-2 text-sm">
    <div class="flex justify-between"><span class="text-slate-400">Integration</span><span>{status.integration_status}</span></div>
    <div class="flex justify-between"><span class="text-slate-400">Last success</span><span>{status.last_successful_backup ?? '—'}</span></div>
    <div class="flex justify-between"><span class="text-slate-400">Last status</span><span>{status.last_backup_status ?? '—'}</span></div>
    <div class="flex justify-between"><span class="text-slate-400">Lag (s)</span><span>{status.backup_lag_seconds ?? '—'}</span></div>
    <div class="flex justify-between"><span class="text-slate-400">WAL</span><span>{status.wal_status ?? '—'}</span></div>
    <div class="flex justify-between"><span class="text-slate-400">Failed backups</span><span>{status.failed_backups}</span></div>
    {#if status.message}<p class="pt-2 text-slate-300">{status.message}</p>{/if}
  </div>

  {#if status.manual_setup_info}
    <div class="card mb-4">
      <h2 class="mb-2 font-medium">Manual setup info</h2>
      <pre class="overflow-x-auto rounded bg-black/30 p-3 text-xs">{JSON.stringify(status.manual_setup_info, null, 2)}</pre>
    </div>
  {/if}
{/if}

<div class="flex gap-2">
  <button class="btn-primary" onclick={register}>Register / retry</button>
  <button class="btn-secondary" onclick={trigger}>Trigger backup</button>
</div>
