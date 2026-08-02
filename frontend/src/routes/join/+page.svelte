<script lang="ts">
	import { page } from '$app/stores';
	import { goto } from '$app/navigation';
	import { api } from '$lib/api';
	import PageHeader from '$lib/components/page-header.svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Input } from '$lib/components/ui/input/index.js';
	import { Label } from '$lib/components/ui/label/index.js';
	import { Textarea } from '$lib/components/ui/textarea/index.js';
	import * as Card from '$lib/components/ui/card/index.js';
	import * as Alert from '$lib/components/ui/alert/index.js';
	import { toast } from 'svelte-sonner';
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import { Copy01Icon, InformationCircleIcon, Link01Icon } from '@hugeicons/core-free-icons';

	let joinLink = $state('');
	let joining = $state(false);

	const inviteToken = $derived($page.url.searchParams.get('token'));
	const inviteUrl = $derived(`${$page.url.origin}${$page.url.pathname}${$page.url.search}`);

	async function copyText(text: string, label: string) {
		try {
			await navigator.clipboard.writeText(text);
			toast.success(`${label} copied`);
		} catch {
			toast.error('Could not copy — select manually');
		}
	}

	async function connectPanel(e: Event) {
		e.preventDefault();
		const link = joinLink.trim();
		if (!link) {
			toast.error('Paste a join link first');
			return;
		}
		joining = true;
		try {
			await api('/api/fleet/join', {
				method: 'POST',
				body: JSON.stringify({ link })
			});
			toast.success('Connected to remote panel');
			goto('/nodes');
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Connect failed');
		} finally {
			joining = false;
		}
	}
</script>

<PageHeader
	title="Join fleet"
	description="Pair this PgPanel with another instance"
/>

{#if inviteToken}
	<Alert.Root class="mb-6 border-primary/30 bg-primary/5">
		<HugeiconsIcon icon={InformationCircleIcon} class="size-4" strokeWidth={2} />
		<Alert.Description>
			This is a node invite for <strong>this</strong> panel. Copy the full URL and paste it on your
			other PgPanel under <strong>Fleet → Connect another panel</strong>.
		</Alert.Description>
	</Alert.Root>

	<Card.Root class="page-card mb-6 border-primary/20">
		<Card.Header>
			<Card.Title>Invite link for this panel</Card.Title>
			<Card.Description>Share this URL with the panel you want to connect from</Card.Description>
		</Card.Header>
		<Card.Content class="space-y-3">
			<div class="flex flex-col gap-2 sm:flex-row sm:items-center">
				<Input readonly value={inviteUrl} class="font-mono text-sm" />
				<Button variant="default" class="shrink-0" onclick={() => copyText(inviteUrl, 'Invite URL')}>
					<HugeiconsIcon icon={Copy01Icon} class="size-4" strokeWidth={2} />
					Copy URL
				</Button>
			</div>
			<p class="text-xs text-muted-foreground">
				Token: <span class="font-mono">{inviteToken}</span>
			</p>
		</Card.Content>
	</Card.Root>
{/if}

<Card.Root class="page-card">
	<Card.Header>
		<Card.Title class="flex items-center gap-2">
			<HugeiconsIcon icon={Link01Icon} class="size-5" strokeWidth={2} />
			Connect from this panel
		</Card.Title>
		<Card.Description>Paste a join link from another PgPanel to pair with it</Card.Description>
	</Card.Header>
	<Card.Content>
		<form class="space-y-4" onsubmit={connectPanel}>
			<div class="space-y-2">
				<Label for="remote-join-link">Remote join link</Label>
				<Textarea
					id="remote-join-link"
					bind:value={joinLink}
					rows={3}
					placeholder="https://other-panel.example.com/join?token=fleet_…"
					class="font-mono text-sm"
				/>
			</div>
			<div class="flex flex-wrap gap-2">
				<Button type="submit" disabled={joining || !joinLink.trim()}>
					{joining ? 'Connecting…' : 'Connect'}
				</Button>
				<Button type="button" variant="outline" href="/nodes">Go to Fleet</Button>
			</div>
		</form>
	</Card.Content>
</Card.Root>
