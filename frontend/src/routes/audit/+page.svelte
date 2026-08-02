<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '$lib/api';

  interface AuditLog {
    id: number;
    actor_username: string | null;
    action: string;
    resource_type: string;
    resource_id: string | null;
    details: unknown;
    created_at: string;
  }

  let logs = $state<AuditLog[]>([]);
  let error = $state('');

  onMount(() => {
    api<AuditLog[]>('/api/audit-logs')
      .then((l) => (logs = l))
      .catch((e) => (error = e.message));
  });
</script>

<h1 class="mb-6 text-2xl font-semibold">Audit log</h1>
{#if error}<p class="text-rose-300">{error}</p>{/if}

<div class="card overflow-x-auto">
  <table class="w-full text-sm">
    <thead class="text-xs uppercase text-slate-500">
      <tr>
        <th class="py-2 text-left">When</th>
        <th class="text-left">Actor</th>
        <th class="text-left">Action</th>
        <th class="text-left">Resource</th>
      </tr>
    </thead>
    <tbody>
      {#each logs as log}
        <tr class="border-t border-slate-800">
          <td class="py-2 text-xs text-slate-500">{log.created_at}</td>
          <td>{log.actor_username ?? '—'}</td>
          <td class="font-mono text-xs">{log.action}</td>
          <td class="text-xs">{log.resource_type} {log.resource_id ?? ''}</td>
        </tr>
      {:else}
        <tr><td class="py-4 text-slate-500" colspan="4">No audit entries</td></tr>
      {/each}
    </tbody>
  </table>
</div>
