<script lang="ts">
  import { goto } from '$app/navigation';
  import { api } from '$lib/api';
  import { login } from '$lib/auth';

  let username = $state('');
  let password = $state('');
  let error = $state('');
  let loading = $state(false);

  onMountCheck();

  async function onMountCheck() {
    try {
      const s = await api<{ bootstrap_required: boolean }>('/api/auth/status');
      if (s.bootstrap_required) goto('/setup');
    } catch {
      /* ignore */
    }
  }

  async function submit(e: Event) {
    e.preventDefault();
    loading = true;
    error = '';
    try {
      await login(username, password);
      goto('/dashboard');
    } catch (err) {
      error = err instanceof Error ? err.message : 'Login failed';
    } finally {
      loading = false;
    }
  }
</script>

<div class="flex min-h-screen items-center justify-center p-4">
  <form class="card w-full max-w-md space-y-4" onsubmit={submit}>
    <div>
      <h1 class="text-xl font-semibold">Sign in to PgPanel</h1>
      <p class="mt-1 text-sm text-slate-400">Server-side session, no JWT in the browser.</p>
    </div>
    {#if error}
      <div class="rounded-lg border border-rose-800 bg-rose-950/50 px-3 py-2 text-sm text-rose-200">{error}</div>
    {/if}
    <div>
      <label class="label" for="username">Username</label>
      <input id="username" class="input" bind:value={username} autocomplete="username" required />
    </div>
    <div>
      <label class="label" for="password">Password</label>
      <input id="password" type="password" class="input" bind:value={password} autocomplete="current-password" required />
    </div>
    <button class="btn-primary w-full" disabled={loading} type="submit">
      {loading ? 'Signing in…' : 'Sign in'}
    </button>
  </form>
</div>
