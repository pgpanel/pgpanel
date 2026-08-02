<script lang="ts">
	import { onMount } from 'svelte';
	import { api } from '$lib/api';
	import PageHeader from '$lib/components/page-header.svelte';
	import * as Card from '$lib/components/ui/card/index.js';
	import { Badge } from '$lib/components/ui/badge/index.js';
	import { Separator } from '$lib/components/ui/separator/index.js';
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import { CheckmarkCircle02Icon, Alert02Icon } from '@hugeicons/core-free-icons';

	let health = $state<{ status: string; version: string } | null>(null);
	let ready = $state<{ ready: boolean; database: boolean; docker: boolean } | null>(null);

	onMount(async () => {
		try {
			health = await api('/health');
			ready = await api('/ready');
		} catch {
			/* ignore */
		}
	});
</script>

<PageHeader title="Settings" description="Panel health and security posture" />

<div class="grid gap-4 md:grid-cols-2">
	<Card.Root class="border-border/60">
		<Card.Header>
			<Card.Title>Panel health</Card.Title>
			<Card.Description>Local process status</Card.Description>
		</Card.Header>
		<Card.Content class="space-y-3 text-sm">
			<div class="flex items-center justify-between">
				<span class="text-muted-foreground">API status</span>
				{#if health}
					<Badge variant={health.status === 'ok' ? 'default' : 'destructive'}>{health.status}</Badge>
				{:else}
					<span>—</span>
				{/if}
			</div>
			<Separator />
			<div class="flex items-center justify-between">
				<span class="text-muted-foreground">Version</span>
				<span class="font-mono text-xs">{health?.version ?? '—'}</span>
			</div>
			<Separator />
			<div class="flex items-center justify-between">
				<span class="text-muted-foreground">SQLite</span>
				{#if ready}
					<span class="inline-flex items-center gap-1.5">
						{#if ready.database}
							<HugeiconsIcon icon={CheckmarkCircle02Icon} class="size-4 text-primary" strokeWidth={2} />
							ok
						{:else}
							<HugeiconsIcon icon={Alert02Icon} class="size-4 text-destructive" strokeWidth={2} />
							down
						{/if}
					</span>
				{:else}
					—
				{/if}
			</div>
			<Separator />
			<div class="flex items-center justify-between">
				<span class="text-muted-foreground">Docker</span>
				{#if ready}
					<span class="inline-flex items-center gap-1.5">
						{#if ready.docker}
							<HugeiconsIcon icon={CheckmarkCircle02Icon} class="size-4 text-primary" strokeWidth={2} />
							ok
						{:else}
							<HugeiconsIcon icon={Alert02Icon} class="size-4 text-destructive" strokeWidth={2} />
							down
						{/if}
					</span>
				{:else}
					—
				{/if}
			</div>
		</Card.Content>
	</Card.Root>

	<Card.Root class="border-border/60">
		<Card.Header>
			<Card.Title>Security notes</Card.Title>
			<Card.Description>Hardening reminders for operators</Card.Description>
		</Card.Header>
		<Card.Content>
			<ul class="space-y-3 text-sm text-muted-foreground">
				<li class="flex gap-2">
					<span class="mt-1 size-1.5 shrink-0 rounded-full bg-primary"></span>
					Admin password is Argon2id-hashed; sessions are server-side cookies.
				</li>
				<li class="flex gap-2">
					<span class="mt-1 size-1.5 shrink-0 rounded-full bg-primary"></span>
					PostgreSQL credentials are AES-256-GCM encrypted at rest.
				</li>
				<li class="flex gap-2">
					<span class="mt-1 size-1.5 shrink-0 rounded-full bg-primary"></span>
					Cookies: HttpOnly, SameSite, Secure in production.
				</li>
				<li class="flex gap-2">
					<span class="mt-1 size-1.5 shrink-0 rounded-full bg-amber-400"></span>
					Docker socket on the panel is a privileged risk — see SECURITY.md.
				</li>
			</ul>
		</Card.Content>
	</Card.Root>
</div>
