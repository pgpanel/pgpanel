<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { api, type Cluster } from '$lib/api';
	import PageHeader from '$lib/components/page-header.svelte';
	import StatusBadge from '$lib/components/status-badge.svelte';
	import EmptyState from '$lib/components/empty-state.svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import * as Card from '$lib/components/ui/card/index.js';
	import * as Table from '$lib/components/ui/table/index.js';
	import * as Alert from '$lib/components/ui/alert/index.js';
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import { PlusSignIcon, Alert02Icon } from '@hugeicons/core-free-icons';

	let clusters = $state<Cluster[]>([]);
	let error = $state('');
	let loading = $state(true);

	onMount(async () => {
		try {
			clusters = await api<Cluster[]>('/api/clusters');
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed';
		} finally {
			loading = false;
		}
	});
</script>

<PageHeader title="Clusters" description="Docker-managed PostgreSQL clusters">
	{#snippet actions()}
		<Button href="/clusters/new">
			<HugeiconsIcon icon={PlusSignIcon} class="size-4" strokeWidth={2} />
			New cluster
		</Button>
	{/snippet}
</PageHeader>

{#if error}
	<Alert.Root variant="destructive" class="mb-4">
		<HugeiconsIcon icon={Alert02Icon} class="size-4" strokeWidth={2} />
		<Alert.Description>{error}</Alert.Description>
	</Alert.Root>
{/if}

<Card.Root class="border-border/60">
	{#if !loading && clusters.length === 0}
		<Card.Content class="pt-6">
			<EmptyState
				title="No clusters"
				description="Create an isolated PostgreSQL 16/17/18 instance with private networking."
			>
				{#snippet action()}
					<Button href="/clusters/new">Create cluster</Button>
				{/snippet}
			</EmptyState>
		</Card.Content>
	{:else}
		<Card.Content class="p-0">
			<Table.Root>
				<Table.Header>
					<Table.Row class="hover:bg-transparent">
						<Table.Head>Name</Table.Head>
						<Table.Head>Version</Table.Head>
						<Table.Head>Status</Table.Head>
						<Table.Head>Resources</Table.Head>
						<Table.Head>Backup</Table.Head>
					</Table.Row>
				</Table.Header>
				<Table.Body>
					{#each clusters as c (c.id)}
						<Table.Row class="cursor-pointer" onclick={() => goto(`/clusters/${c.id}`)}>
							<Table.Cell>
								<a
									class="font-medium text-primary hover:underline"
									href={`/clusters/${c.id}`}
									onclick={(e) => e.stopPropagation()}>{c.name}</a
								>
								<div class="font-mono text-xs text-muted-foreground">{c.slug}</div>
							</Table.Cell>
							<Table.Cell>
								<span class="rounded-md bg-muted px-2 py-0.5 font-mono text-xs">PG {c.postgres_version}</span>
							</Table.Cell>
							<Table.Cell><StatusBadge status={c.status} /></Table.Cell>
							<Table.Cell class="text-muted-foreground">
								{c.cpu_limit} CPU · {c.memory_mb} MB · {c.storage_limit_gb} GB
							</Table.Cell>
							<Table.Cell class="text-muted-foreground">
								{c.enable_backup ? 'on' : 'off'}
							</Table.Cell>
						</Table.Row>
					{/each}
				</Table.Body>
			</Table.Root>
		</Card.Content>
	{/if}
</Card.Root>
