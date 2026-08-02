<script lang="ts">
  import '../app.css';
  import { onMount } from 'svelte';
  import { page } from '$app/stores';
  import { goto } from '$app/navigation';
  import { authReady, refreshMe, user, logout } from '$lib/auth';

  let { children } = $props();

  const publicPaths = ['/login', '/setup'];

  onMount(async () => {
    await refreshMe();
  });

  $effect(() => {
    if (!$authReady) return;
    const path = $page.url.pathname;
    if (!$user && !publicPaths.includes(path)) {
      goto('/login');
    }
    if ($user && (path === '/login' || path === '/setup')) {
      goto('/dashboard');
    }
  });
</script>

{#if !$authReady}
  <div class="flex min-h-screen items-center justify-center text-slate-400">Loading…</div>
{:else if publicPaths.includes($page.url.pathname)}
  {@render children()}
{:else if $user}
  <div class="min-h-screen lg:grid lg:grid-cols-[240px_1fr]">
    <aside class="border-b border-slate-800 bg-slate-900/60 lg:border-b-0 lg:border-r">
      <div class="flex items-center gap-2 px-5 py-5">
        <div class="h-8 w-8 rounded-lg bg-emerald-600/20 text-center text-lg leading-8 text-emerald-400">P</div>
        <div>
          <div class="font-semibold tracking-tight">PgPanel</div>
          <div class="text-xs text-slate-500">PostgreSQL control plane</div>
        </div>
      </div>
      <nav class="space-y-1 px-3 pb-6 text-sm">
        <a class="nav" href="/dashboard">Dashboard</a>
        <a class="nav" href="/clusters">Clusters</a>
        <a class="nav" href="/clusters/new">New cluster</a>
        <a class="nav" href="/operations">Operations</a>
        <a class="nav" href="/audit">Audit log</a>
        <a class="nav" href="/settings">Settings</a>
      </nav>
      <div class="border-t border-slate-800 px-5 py-4 text-sm text-slate-400">
        <div class="mb-2">{$user.username}</div>
        <button class="btn-secondary w-full" onclick={() => logout().then(() => goto('/login'))}>
          Log out
        </button>
      </div>
    </aside>
    <main class="p-6 lg:p-8">{@render children()}</main>
  </div>
{/if}

<style>
  :global(.nav) {
    display: block;
    border-radius: 0.5rem;
    padding: 0.5rem 0.75rem;
    color: rgb(203 213 225);
  }
  :global(.nav:hover) {
    background: rgb(30 41 59);
    color: white;
  }
</style>
