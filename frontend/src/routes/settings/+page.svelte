<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '$lib/api';

  let health = $state<{ status: string; version: string } | null>(null);
  let ready = $state<{ ready: boolean; database: boolean; docker: boolean } | null>(null);

  onMount(async () => {
    health = await api('/health');
    ready = await api('/ready');
  });
</script>

<h1 class="mb-6 text-2xl font-semibold">Settings</h1>

<div class="grid gap-4 md:grid-cols-2">
  <div class="card text-sm space-y-2">
    <h2 class="font-medium">Panel health</h2>
    {#if health}
      <div class="flex justify-between"><span class="text-slate-400">Status</span><span>{health.status}</span></div>
      <div class="flex justify-between"><span class="text-slate-400">Version</span><span>{health.version}</span></div>
    {/if}
    {#if ready}
      <div class="flex justify-between"><span class="text-slate-400">SQLite</span><span>{ready.database ? 'ok' : 'down'}</span></div>
      <div class="flex justify-between"><span class="text-slate-400">Docker</span><span>{ready.docker ? 'ok' : 'down'}</span></div>
    {/if}
  </div>
  <div class="card text-sm space-y-2">
    <h2 class="font-medium">Security notes</h2>
    <ul class="list-disc space-y-1 pl-5 text-slate-400">
      <li>Admin password is Argon2id-hashed.</li>
      <li>PostgreSQL credentials are AES-256-GCM encrypted at rest.</li>
      <li>Browser sessions are server-side cookies (HttpOnly, SameSite, Secure).</li>
      <li>Docker socket access is a privileged risk — see SECURITY.md.</li>
    </ul>
  </div>
</div>
