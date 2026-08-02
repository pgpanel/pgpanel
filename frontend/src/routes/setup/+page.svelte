<script lang="ts">
	import { goto } from '$app/navigation';
	import { bootstrap } from '$lib/auth';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Input } from '$lib/components/ui/input/index.js';
	import { Label } from '$lib/components/ui/label/index.js';
	import * as Card from '$lib/components/ui/card/index.js';
	import * as Alert from '$lib/components/ui/alert/index.js';
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import { ServerStack01Icon, Alert02Icon, InformationCircleIcon } from '@hugeicons/core-free-icons';

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

<div class="auth-grid flex min-h-screen items-center justify-center p-4">
	<Card.Root class="w-full max-w-md border-border/60 shadow-2xl shadow-black/40">
		<Card.Header class="space-y-3 text-center">
			<div class="mx-auto flex size-12 items-center justify-center rounded-xl bg-primary text-primary-foreground shadow-lg shadow-primary/25">
				<HugeiconsIcon icon={ServerStack01Icon} class="size-6" strokeWidth={2} />
			</div>
			<div>
				<Card.Title class="text-xl">Initial setup</Card.Title>
				<Card.Description>Create the first admin account. This can only be done once.</Card.Description>
			</div>
		</Card.Header>
		<Card.Content>
			<form class="space-y-4" onsubmit={submit}>
				{#if error}
					<Alert.Root variant="destructive">
						<HugeiconsIcon icon={Alert02Icon} class="size-4" strokeWidth={2} />
						<Alert.Title>Setup failed</Alert.Title>
						<Alert.Description>{error}</Alert.Description>
					</Alert.Root>
				{/if}
				<Alert.Root>
					<HugeiconsIcon icon={InformationCircleIcon} class="size-4" strokeWidth={2} />
					<Alert.Title>Security tip</Alert.Title>
					<Alert.Description>
						Use a long password (16+ chars). Prefer the installer-generated password from
						<code class="text-xs">/etc/pgpanel/.admin-password-ONCE</code>.
					</Alert.Description>
				</Alert.Root>
				<div class="space-y-2">
					<Label for="username">Admin username</Label>
					<Input id="username" bind:value={username} required />
				</div>
				<div class="space-y-2">
					<Label for="password">Password</Label>
					<Input id="password" type="password" bind:value={password} required />
				</div>
				<div class="space-y-2">
					<Label for="confirm">Confirm password</Label>
					<Input id="confirm" type="password" bind:value={confirm} required />
				</div>
				<div class="space-y-2">
					<Label for="token">Bootstrap token (optional)</Label>
					<Input id="token" bind:value={bootstrap_token} placeholder="From installer output or .env" />
				</div>
				<Button class="w-full" type="submit" disabled={loading}>
					{loading ? 'Creating…' : 'Create admin'}
				</Button>
			</form>
		</Card.Content>
	</Card.Root>
</div>
