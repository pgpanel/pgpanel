<script lang="ts">
	import { onMount } from 'svelte';
	import { api, type Alert, type AlertRule, formatRelative } from '$lib/api';
	import PageHeader from '$lib/components/page-header.svelte';
	import StatusBadge from '$lib/components/status-badge.svelte';
	import EmptyState from '$lib/components/empty-state.svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Input } from '$lib/components/ui/input/index.js';
	import { Switch } from '$lib/components/ui/switch/index.js';
	import * as Card from '$lib/components/ui/card/index.js';
	import * as Table from '$lib/components/ui/table/index.js';
	import * as AlertUI from '$lib/components/ui/alert/index.js';
	import { Badge } from '$lib/components/ui/badge/index.js';
	import { toast } from 'svelte-sonner';
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import {
		Alert02Icon,
		Notification01Icon,
		RefreshIcon,
		CheckmarkCircle02Icon,
		Settings01Icon
	} from '@hugeicons/core-free-icons';

	let alerts = $state<Alert[]>([]);
	let rules = $state<AlertRule[]>([]);
	let error = $state('');
	let loading = $state(true);
	let savingRule = $state<string | null>(null);

	const openAlerts = $derived(alerts.filter((a) => a.status === 'open'));

	async function load() {
		const [a, r] = await Promise.all([
			api<Alert[]>('/api/alerts'),
			api<AlertRule[]>('/api/alert-rules')
		]);
		alerts = a;
		rules = r;
	}

	onMount(() => {
		load()
			.catch((e) => (error = e instanceof Error ? e.message : 'Failed to load alerts'))
			.finally(() => (loading = false));
	});

	async function ackAlert(id: string) {
		try {
			await api(`/api/alerts/${id}/ack`, { method: 'POST' });
			toast.success('Alert acknowledged');
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Ack failed');
		}
	}

	async function resolveAlert(id: string) {
		try {
			await api(`/api/alerts/${id}/resolve`, { method: 'POST' });
			toast.success('Alert resolved');
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Resolve failed');
		}
	}

	async function updateRule(rule: AlertRule, patch: { enabled?: boolean; threshold?: number }) {
		savingRule = rule.id;
		try {
			const updated = await api<AlertRule>(`/api/alert-rules/${rule.id}`, {
				method: 'PUT',
				body: JSON.stringify(patch)
			});
			rules = rules.map((r) => (r.id === rule.id ? updated : r));
			toast.success('Rule updated');
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Update failed');
		} finally {
			savingRule = null;
		}
	}

	function severityVariant(severity: string) {
		const s = severity.toLowerCase();
		if (s === 'critical' || s === 'error') return 'destructive' as const;
		if (s === 'warning') return 'secondary' as const;
		return 'outline' as const;
	}
</script>

<PageHeader title="Alerts" description="Open incidents and configurable alert rules">
	{#snippet actions()}
		<Button variant="outline" size="sm" href="/monitoring">Monitoring</Button>
		<Button variant="outline" size="sm" onclick={() => load()}>
			<HugeiconsIcon icon={RefreshIcon} class="size-4" strokeWidth={2} />
			Refresh
		</Button>
	{/snippet}
</PageHeader>

{#if error}
	<AlertUI.Root variant="destructive" class="mb-4">
		<HugeiconsIcon icon={Alert02Icon} class="size-4" strokeWidth={2} />
		<AlertUI.Description>{error}</AlertUI.Description>
	</AlertUI.Root>
{/if}

<Card.Root class="mb-6 border-border/60">
	<Card.Header>
		<Card.Title class="flex items-center gap-2">
			<HugeiconsIcon icon={Notification01Icon} class="size-5 text-primary" strokeWidth={2} />
			Open alerts
		</Card.Title>
		<Card.Description>{openAlerts.length} open · {alerts.length} total (last 200)</Card.Description>
	</Card.Header>
	<Card.Content class="p-0">
		{#if !loading && openAlerts.length === 0}
			<div class="p-6">
				<EmptyState title="No open alerts" description="All clear — no active incidents." />
			</div>
		{:else}
			<Table.Root>
				<Table.Header>
					<Table.Row class="hover:bg-transparent">
						<Table.Head>Severity</Table.Head>
						<Table.Head>Alert</Table.Head>
						<Table.Head>Status</Table.Head>
						<Table.Head>Fired</Table.Head>
						<Table.Head class="text-right">Actions</Table.Head>
					</Table.Row>
				</Table.Header>
				<Table.Body>
					{#each openAlerts as a (a.id)}
						<Table.Row>
							<Table.Cell>
								<Badge variant={severityVariant(a.severity)} class="uppercase">{a.severity}</Badge>
							</Table.Cell>
							<Table.Cell>
								<div class="font-medium">{a.title}</div>
								<div class="text-xs text-muted-foreground">{a.message}</div>
								{#if a.resource_type}
									<div class="text-xs text-muted-foreground">
										{a.resource_type}{#if a.resource_id}: {a.resource_id}{/if}
									</div>
								{/if}
							</Table.Cell>
							<Table.Cell>
								<StatusBadge status={a.status} />
							</Table.Cell>
							<Table.Cell class="text-xs whitespace-nowrap">
								{formatRelative(a.fired_at)}
							</Table.Cell>
							<Table.Cell class="text-right">
								<div class="flex justify-end gap-1">
									{#if a.status === 'open'}
										<Button variant="ghost" size="sm" onclick={() => ackAlert(a.id)}>
											Ack
										</Button>
									{/if}
									<Button variant="ghost" size="sm" onclick={() => resolveAlert(a.id)}>
										<HugeiconsIcon icon={CheckmarkCircle02Icon} class="size-4" strokeWidth={2} />
										Resolve
									</Button>
								</div>
							</Table.Cell>
						</Table.Row>
					{/each}
				</Table.Body>
			</Table.Root>
		{/if}
	</Card.Content>
</Card.Root>

<Card.Root class="border-border/60">
	<Card.Header>
		<Card.Title class="flex items-center gap-2">
			<HugeiconsIcon icon={Settings01Icon} class="size-5 text-primary" strokeWidth={2} />
			Alert rules
		</Card.Title>
		<Card.Description>Toggle rules and adjust thresholds</Card.Description>
	</Card.Header>
	<Card.Content class="p-0">
		<Table.Root>
			<Table.Header>
				<Table.Row class="hover:bg-transparent">
					<Table.Head>Name</Table.Head>
					<Table.Head>Metric</Table.Head>
					<Table.Head>Threshold</Table.Head>
					<Table.Head>Enabled</Table.Head>
				</Table.Row>
			</Table.Header>
			<Table.Body>
				{#each rules as rule (rule.id)}
					<Table.Row>
						<Table.Cell>
							<div class="font-medium">{rule.name}</div>
							<div class="text-xs text-muted-foreground">
								{rule.severity} · {rule.scope}
							</div>
						</Table.Cell>
						<Table.Cell class="font-mono text-xs">
							{rule.metric} {rule.operator}
						</Table.Cell>
						<Table.Cell>
							<div class="flex items-center gap-2">
								<Input
									type="number"
									step="0.1"
									class="h-8 w-24 font-mono text-xs"
									value={rule.threshold}
									disabled={savingRule === rule.id}
									onchange={(e) => {
										const v = parseFloat(e.currentTarget.value);
										if (!Number.isNaN(v)) updateRule(rule, { threshold: v });
									}}
								/>
								<span class="text-xs text-muted-foreground">{rule.duration_seconds}s</span>
							</div>
						</Table.Cell>
						<Table.Cell>
							<Switch
								checked={rule.enabled}
								disabled={savingRule === rule.id}
								onclick={() => updateRule(rule, { enabled: !rule.enabled })}
							/>
						</Table.Cell>
					</Table.Row>
				{:else}
					<Table.Row>
						<Table.Cell colspan={4} class="py-8 text-center text-sm text-muted-foreground">
							No alert rules configured.
						</Table.Cell>
					</Table.Row>
				{/each}
			</Table.Body>
		</Table.Root>
	</Card.Content>
</Card.Root>
