<script lang="ts">
	import { onMount } from 'svelte';
	import { api, type AuditLog } from '$lib/api';
	import PageHeader from '$lib/components/page-header.svelte';
	import EmptyState from '$lib/components/empty-state.svelte';
	import * as Card from '$lib/components/ui/card/index.js';
	import * as Table from '$lib/components/ui/table/index.js';
	import * as Alert from '$lib/components/ui/alert/index.js';
	import { Badge } from '$lib/components/ui/badge/index.js';
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import { Alert02Icon } from '@hugeicons/core-free-icons';

	let logs = $state<AuditLog[]>([]);
	let error = $state('');

	onMount(() => {
		api<AuditLog[]>('/api/audit-logs')
			.then((l) => (logs = l))
			.catch((e) => (error = e.message));
	});
</script>

<PageHeader
	title="Audit log"
	description="Dangerous operations: create/delete cluster, passwords, port publish, backups"
/>

{#if error}
	<Alert.Root variant="destructive" class="mb-4">
		<HugeiconsIcon icon={Alert02Icon} class="size-4" strokeWidth={2} />
		<Alert.Description>{error}</Alert.Description>
	</Alert.Root>
{/if}

<Card.Root class="border-border/60">
	{#if logs.length === 0}
		<Card.Content class="pt-6">
			<EmptyState title="No audit entries" description="Actions will be recorded as you manage clusters." />
		</Card.Content>
	{:else}
		<Card.Content class="p-0">
			<Table.Root>
				<Table.Header>
					<Table.Row>
						<Table.Head>When</Table.Head>
						<Table.Head>Actor</Table.Head>
						<Table.Head>Action</Table.Head>
						<Table.Head>Resource</Table.Head>
					</Table.Row>
				</Table.Header>
				<Table.Body>
					{#each logs as log}
						<Table.Row>
							<Table.Cell class="whitespace-nowrap text-xs text-muted-foreground">
								{log.created_at}
							</Table.Cell>
							<Table.Cell>{log.actor_username ?? '—'}</Table.Cell>
							<Table.Cell>
								<Badge variant="outline" class="font-mono text-[11px]">{log.action}</Badge>
							</Table.Cell>
							<Table.Cell class="text-xs">
								<span class="text-muted-foreground">{log.resource_type}</span>
								{#if log.resource_id}
									<span class="ml-1 font-mono">{log.resource_id}</span>
								{/if}
							</Table.Cell>
						</Table.Row>
					{/each}
				</Table.Body>
			</Table.Root>
		</Card.Content>
	{/if}
</Card.Root>
