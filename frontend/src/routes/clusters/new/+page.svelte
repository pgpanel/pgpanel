<script lang="ts">
  import { goto } from '$app/navigation';
  import { api } from '$lib/api';

  let name = $state('');
  let postgres_version = $state('17');
  let cpu_limit = $state(1);
  let memory_mb = $state(1024);
  let storage_limit_gb = $state(20);
  let expose_publicly = $state(false);
  let optional_public_port = $state(5433);
  let enable_backup = $state(true);
  let error = $state('');
  let oneTimePassword = $state('');
  let loading = $state(false);

  async function submit(e: Event) {
    e.preventDefault();
    loading = true;
    error = '';
    try {
      const res = await api<{
        cluster: { id: string };
        admin_password: string;
        operation_id: string;
      }>('/api/clusters', {
        method: 'POST',
        body: JSON.stringify({
          name,
          postgres_version,
          cpu_limit,
          memory_mb,
          storage_limit_gb,
          expose_publicly,
          optional_public_port: expose_publicly ? optional_public_port : null,
          enable_backup
        })
      });
      oneTimePassword = res.admin_password;
      setTimeout(() => goto(`/clusters/${res.cluster.id}`), 4000);
    } catch (err) {
      error = err instanceof Error ? err.message : 'Create failed';
    } finally {
      loading = false;
    }
  }
</script>

<h1 class="mb-6 text-2xl font-semibold">Create cluster</h1>

{#if oneTimePassword}
  <div class="card mb-6 border-amber-700/50 bg-amber-950/30">
    <h2 class="font-medium text-amber-200">One-time admin password</h2>
    <p class="mt-1 text-sm text-amber-100/80">Copy now — it will not be shown again.</p>
    <code class="mt-3 block break-all rounded bg-black/40 p-3 font-mono text-sm">{oneTimePassword}</code>
  </div>
{/if}

<form class="card max-w-xl space-y-4" onsubmit={submit}>
  {#if error}<div class="text-sm text-rose-300">{error}</div>{/if}
  <div>
    <label class="label" for="name">Display name</label>
    <input id="name" class="input" bind:value={name} placeholder="Production Primary" required />
  </div>
  <div>
    <label class="label" for="ver">PostgreSQL version</label>
    <select id="ver" class="input" bind:value={postgres_version}>
      <option value="16">16</option>
      <option value="17">17</option>
      <option value="18">18</option>
    </select>
  </div>
  <div class="grid grid-cols-3 gap-3">
    <div>
      <label class="label" for="cpu">CPU</label>
      <input id="cpu" type="number" step="0.1" min="0.1" class="input" bind:value={cpu_limit} />
    </div>
    <div>
      <label class="label" for="mem">Memory MB</label>
      <input id="mem" type="number" min="128" class="input" bind:value={memory_mb} />
    </div>
    <div>
      <label class="label" for="disk">Storage GB</label>
      <input id="disk" type="number" min="1" class="input" bind:value={storage_limit_gb} />
    </div>
  </div>
  <label class="flex items-center gap-2 text-sm">
    <input type="checkbox" bind:checked={enable_backup} /> Enable Databasus backup integration
  </label>
  <label class="flex items-center gap-2 text-sm">
    <input type="checkbox" bind:checked={expose_publicly} /> Expose public port (disabled by default)
  </label>
  {#if expose_publicly}
    <div>
      <label class="label" for="port">Public port</label>
      <input id="port" type="number" min="1024" class="input" bind:value={optional_public_port} />
    </div>
  {/if}
  <button class="btn-primary" disabled={loading} type="submit">{loading ? 'Creating…' : 'Create cluster'}</button>
</form>
