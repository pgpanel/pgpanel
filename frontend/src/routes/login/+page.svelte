<script lang="ts">
	import { goto } from '$app/navigation';
	import { api } from '$lib/api';
	import { login } from '$lib/auth';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Input } from '$lib/components/ui/input/index.js';
	import { Label } from '$lib/components/ui/label/index.js';
	import * as Card from '$lib/components/ui/card/index.js';
	import * as Alert from '$lib/components/ui/alert/index.js';
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import { ServerStack01Icon, Alert02Icon } from '@hugeicons/core-free-icons';

	let username = $state('');
	let password = $state('');
	let error = $state('');
	let loading = $state(false);

	async function checkBootstrap() {
		try {
			const s = await api<{ bootstrap_required: boolean }>('/api/auth/status');
			if (s.bootstrap_required) goto('/setup');
		} catch {
			/* ignore */
		}
	}
	checkBootstrap();

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

<div class="auth-grid flex min-h-screen items-center justify-center p-4">
	<Card.Root class="w-full max-w-md border-border/60 shadow-2xl shadow-black/40">
		<Card.Header class="space-y-3 text-center">
			<div class="mx-auto flex size-12 items-center justify-center rounded-xl bg-primary text-primary-foreground shadow-lg shadow-primary/25">
				<HugeiconsIcon icon={ServerStack01Icon} class="size-6" strokeWidth={2} />
			</div>
			<div>
				<Card.Title class="text-xl">Sign in to PgPanel</Card.Title>
				<Card.Description>Server-side session · CSRF protected · no browser JWT</Card.Description>
			</div>
		</Card.Header>
		<Card.Content>
			<form class="space-y-4" onsubmit={submit}>
				{#if error}
					<Alert.Root variant="destructive">
						<HugeiconsIcon icon={Alert02Icon} class="size-4" strokeWidth={2} />
						<Alert.Title>Sign-in failed</Alert.Title>
						<Alert.Description>{error}</Alert.Description>
					</Alert.Root>
				{/if}
				<div class="space-y-2">
					<Label for="username">Username</Label>
					<Input id="username" bind:value={username} autocomplete="username" required placeholder="admin" />
				</div>
				<div class="space-y-2">
					<Label for="password">Password</Label>
					<Input
						id="password"
						type="password"
						bind:value={password}
						autocomplete="current-password"
						required
						placeholder="••••••••••••••••"
					/>
				</div>
				<Button class="w-full" type="submit" disabled={loading}>
					{loading ? 'Signing in…' : 'Sign in'}
				</Button>
			</form>
		</Card.Content>
	</Card.Root>
</div>
