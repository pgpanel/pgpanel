<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/stores';
	import { api, type DatabaseRecord } from '$lib/api';
	import PageHeader from '$lib/components/page-header.svelte';
	import EmptyState from '$lib/components/empty-state.svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Input } from '$lib/components/ui/input/index.js';
	import { Label } from '$lib/components/ui/label/index.js';
	import * as Card from '$lib/components/ui/card/index.js';
	import * as Table from '$lib/components/ui/table/index.js';
	import * as Alert from '$lib/components/ui/alert/index.js';
	import { toast } from 'svelte-sonner';
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import { ArrowLeft01Icon, Alert02Icon } from '@hugeicons/core-free-icons';

	let databases = $state<DatabaseRecord[]>([]);
	let database_name = $state('');
	let role_name = $state('');
	let error = $state('');
	let loading = $state(false);

	const id = $derived($page.params.id);
	const namePattern = '^[a-z][a-z0-9_]{2,62}$';

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
			toast.success('Database creation queued', { description: res.operation_id });
			database_name = '';
			role_name = '';
			await load();
		} catch (err) {
			error = err instanceof Error ? err.message : 'Failed';
			toast.error(error);
		} finally {
			loading = false;
		}
	}
</script>

<div class="mb-2">
	<Button variant="ghost" size="sm" href={`/clusters/${id}`}>
		<HugeiconsIcon icon={ArrowLeft01Icon} class="size-4" strokeWidth={2} />
		Back to cluster
	</Button>
</div>

<PageHeader
	title="Databases & roles"
	description="Create application databases with non-superuser owners"
/>

{#if error}
	<Alert.Root variant="destructive" class="mb-4">
		<HugeiconsIcon icon={Alert02Icon} class="size-4" strokeWidth={2} />
		<Alert.Description>{error}</Alert.Description>
	</Alert.Root>
{/if}

<div class="grid gap-6 lg:grid-cols-[360px_1fr]">
	<Card.Root class="border-border/60 h-fit">
		<Card.Header>
			<Card.Title>Create database</Card.Title>
			<Card.Description>Names must match ^[a-z][a-z0-9_]{'{2,62}'}$</Card.Description>
		</Card.Header>
		<form onsubmit={create}>
			<Card.Content class="space-y-4">
				<div class="space-y-2">
					<Label for="db">Database name</Label>
					<Input id="db" bind:value={database_name} pattern={namePattern} required class="font-mono" />
				</div>
				<div class="space-y-2">
					<Label for="role">Role name</Label>
					<Input id="role" bind:value={role_name} pattern={namePattern} required class="font-mono" />
				</div>
			</Card.Content>
			<Card.Footer>
				<Button type="submit" class="w-full" disabled={loading}>
					{loading ? 'Creating…' : 'Create with generated password'}
				</Button>
			</Card.Footer>
		</form>
	</Card.Root>

	<Card.Root class="border-border/60">
		<Card.Header>
			<Card.Title>Tracked databases</Card.Title>
		</Card.Header>
		{#if databases.length === 0}
			<Card.Content>
				<EmptyState title="No application databases" description="Create one to get a one-time connection string in the operation result." />
			</Card.Content>
		{:else}
			<Card.Content class="p-0">
				<Table.Root>
					<Table.Header>
						<Table.Row>
							<Table.Head>Database</Table.Head>
							<Table.Head>Owner role</Table.Head>
						</Table.Row>
					</Table.Header>
					<Table.Body>
						{#each databases as d}
							<Table.Row>
								<Table.Cell class="font-mono">{d.name}</Table.Cell>
								<Table.Cell class="font-mono">{d.owner_role}</Table.Cell>
							</Table.Row>
						{/each}
					</Table.Body>
				</Table.Root>
			</Card.Content>
		{/if}
	</Card.Root>
</div>
