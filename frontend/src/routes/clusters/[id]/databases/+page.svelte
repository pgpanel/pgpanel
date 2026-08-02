<script lang="ts">
  import { onMount } from 'svelte';
  import { page } from '$app/stores';
  import { api, type DatabaseRecord } from '$lib/api';

  let databases = $state<DatabaseRecord[]>([]);
  let database_name = $state('');
  let role_name = $state('');
  let error = $state('');
  let message = $state('');
  let loading = $state(false);

  const id = $derived($page.params.id);

  async function load() {
    databases = await api<DatabaseRecord[]>(`/api/clusters/${id}/databases`);
  }

  onMount(() => {
    load().catch((e) => (error = e.message));
  });

  async function create(e: Event) {
    e.preventDefault();
    loading = true;
    error = '';
    message = '';
    try {
      const res = await api<{ operation_id: string; message: string }>(
        `/api/clusters/${id}/databases`,
        {
          method: 'POST',
          body: JSON.stringify({
            database_name,
            role_name,
            generate_password: true
          })
        }
      );
      message = `${res.message} Operation: ${res.operation_id}`;
      database_name = '';
      role_name = '';
      await load();
    } catch (err) {
      error = err instanceof Error ? err.message : 'Failed';
    } finally {
      loading = false;
    }
  }
</script>

<div class="mb-4">
  <a class="text-sm text-slate-400 hover:text-white" href={`/clusters/${id}`}>← Cluster</a>
  <h1 class="mt-2 text-2xl font-semibold">Databases & roles</h1>
</div>

{#if error}<p class="mb-3 text-sm text-rose-300">{error}</p>{/if}
{#if message}<p class="mb-3 text-sm text-emerald-300">{message}</p>{/if}

<form class="card mb-6 max-w-lg space-y-3" onsubmit={create}>
  <h2 class="font-medium">Create database + role</h2>
  <div>
    <label class="label" for="db">Database name</label>
    <input id="db" class="input" bind:value={database_name} pattern={"[a-z][a-z0-9_]{2,62}"} required />
  </div>
  <div>
    <label class="label" for="role">Role name</label>
    <input id="role" class="input" bind:value={role_name} pattern={"[a-z][a-z0-9_]{2,62}"} required />
  </div>
  <button class="btn-primary" disabled={loading} type="submit">Create</button>
</form>

<div class="card">
  <table class="w-full text-sm">
    <thead class="text-xs uppercase text-slate-500">
      <tr><th class="py-2 text-left">Database</th><th class="text-left">Owner</th></tr>
    </thead>
    <tbody>
      {#each databases as d}
        <tr class="border-t border-slate-800">
          <td class="py-2 font-mono">{d.name}</td>
          <td class="font-mono">{d.owner_role}</td>
        </tr>
      {:else}
        <tr><td class="py-4 text-slate-500" colspan="2">No application databases tracked yet.</td></tr>
      {/each}
    </tbody>
  </table>
</div>
