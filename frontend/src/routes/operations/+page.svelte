<script lang="ts">
	import { onMount } from 'svelte';
	import { api, type Operation } from '$lib/api';
	import PageHeader from '$lib/components/page-header.svelte';
	import StatusBadge from '$lib/components/status-badge.svelte';
	import EmptyState from '$lib/components/empty-state.svelte';
	import * as Card from '$lib/components/ui/card/index.js';
	import * as Table from '$lib/components/ui/table/index.js';
	import { Progress } from '$lib/components/ui/progress/index.js';
	import * as Alert from '$lib/components/ui/alert/index.js';
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import { Alert02Icon } from '@hugeicons/core-free-icons';

	let ops = $state<Operation[]>([]);
	let error = $state('');

	onMount(() => {
		const load = () =>
			api<Operation[]>('/api/operations')
				.then((o) => (ops = o))
				.catch((e) => (error = e.message));
		load();
		const t = setInterval(load, 3000);
		return () => clearInterval(t);
	});
</script>

<PageHeader title="Operations" description="Durable job queue · auto-refresh 3s" />

{#if error}
	<Alert.Root variant="destructive" class="mb-4">
		<HugeiconsIcon icon={Alert02Icon} class="size-4" strokeWidth={2} />
		<Alert.Description>{error}</Alert.Description>
	</Alert.Root>
{/if}

<Card.Root class="border-border/60">
	{#if ops.length === 0}
		<Card.Content class="pt-6">
			<EmptyState title="No operations" description="Cluster and database jobs will appear here." />
		</Card.Content>
	{:else}
		<Card.Content class="p-0">
			<Table.Root>
				<Table.Header>
					<Table.Row>
						<Table.Head>Type</Table.Head>
						<Table.Head>Status</Table.Head>
						<Table.Head class="w-[180px]">Progress</Table.Head>
						<Table.Head>Error</Table.Head>
						<Table.Head>Updated</Table.Head>
					</Table.Row>
				</Table.Header>
				<Table.Body>
					{#each ops as op}
						<Table.Row>
							<Table.Cell>
								<div class="font-mono text-xs font-medium">{op.job_type}</div>
								<div class="text-[11px] text-muted-foreground">{op.id.slice(0, 8)}</div>
							</Table.Cell>
							<Table.Cell><StatusBadge status={op.status} /></Table.Cell>
							<Table.Cell>
								<div class="flex items-center gap-2">
									<Progress value={op.progress} class="h-1.5 flex-1" />
									<span class="w-8 text-right text-xs tabular-nums text-muted-foreground"
										>{op.progress}%</span
									>
								</div>
							</Table.Cell>
							<Table.Cell class="max-w-[240px] truncate text-xs text-destructive">
								{op.error ?? ''}
							</Table.Cell>
							<Table.Cell class="text-xs text-muted-foreground whitespace-nowrap">
								{op.updated_at}
							</Table.Cell>
						</Table.Row>
					{/each}
				</Table.Body>
			</Table.Root>
		</Card.Content>
	{/if}
</Card.Root>
