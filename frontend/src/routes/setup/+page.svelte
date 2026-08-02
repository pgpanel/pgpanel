<script lang="ts">
  import { goto } from '$app/navigation';
  import { bootstrap } from '$lib/auth';

  let username = $state('admin');
  let password = $state('');
  let confirm = $state('');
  let bootstrap_token = $state('');
  let error = $state('');
  let loading = $state(false);

  async function submit(e: Event) {
    e.preventDefault();
    if (password !== confirm) {
      error = 'Passwords do not match';
      return;
    }
    if (password.length < 16) {
      error = 'Password must be at least 16 characters';
      return;
    }
    loading = true;
    error = '';
    try {
      await bootstrap(username, password, bootstrap_token || undefined);
      goto('/dashboard');
    } catch (err) {
      error = err instanceof Error ? err.message : 'Bootstrap failed';
    } finally {
      loading = false;
    }
  }
</script>

<div class="flex min-h-screen items-center justify-center p-4">
  <form class="card w-full max-w-md space-y-4" onsubmit={submit}>
    <div>
      <h1 class="text-xl font-semibold">Initial setup</h1>
      <p class="mt-1 text-sm text-slate-400">Create the first admin account. This can only be done once.</p>
    </div>
    {#if error}
      <div class="rounded-lg border border-rose-800 bg-rose-950/50 px-3 py-2 text-sm text-rose-200">{error}</div>
    {/if}
    <div>
      <label class="label" for="username">Admin username</label>
      <input id="username" class="input" bind:value={username} required />
    </div>
    <div>
      <label class="label" for="password">Password (min 16 chars)</label>
      <input id="password" type="password" class="input" bind:value={password} required />
    </div>
    <div>
      <label class="label" for="confirm">Confirm password</label>
      <input id="confirm" type="password" class="input" bind:value={confirm} required />
    </div>
    <div>
      <label class="label" for="token">Bootstrap token (optional)</label>
      <input id="token" class="input" bind:value={bootstrap_token} />
    </div>
    <button class="btn-primary w-full" disabled={loading} type="submit">
      {loading ? 'Creating…' : 'Create admin'}
    </button>
  </form>
</div>
