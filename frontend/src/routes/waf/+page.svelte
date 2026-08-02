<script lang="ts">
	import { onMount } from 'svelte';
	import { api, type WafPolicy, type WafPolicyConfig, type WafChangeEntry, formatRelative } from '$lib/api';
	import PageHeader from '$lib/components/page-header.svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Input } from '$lib/components/ui/input/index.js';
	import { Label } from '$lib/components/ui/label/index.js';
	import { Switch } from '$lib/components/ui/switch/index.js';
	import { Textarea } from '$lib/components/ui/textarea/index.js';
	import * as Card from '$lib/components/ui/card/index.js';
	import * as Table from '$lib/components/ui/table/index.js';
	import * as Alert from '$lib/components/ui/alert/index.js';
	import * as Select from '$lib/components/ui/select/index.js';
	import { Badge } from '$lib/components/ui/badge/index.js';
	import { toast } from 'svelte-sonner';
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import {
		Alert02Icon,
		SecurityLockIcon,
		RefreshIcon,
		SaveEnergy01Icon,
		SourceCodeIcon
	} from '@hugeicons/core-free-icons';

	let policy = $state<WafPolicy | null>(null);
	let changes = $state<WafChangeEntry[]>([]);
	let error = $state('');
	let saving = $state(false);
	let caddySnippet = $state('');
	let showSnippet = $state(false);

	let reason = $state('');

	let rate_limit_per_minute = $state(120);
	let login_rate_limit_per_minute = $state(20);
	let api_rate_limit_per_minute = $state(300);
	let max_body_bytes = $state(1048576);
	let block_empty_user_agent = $state(false);
	let blocked_user_agents = $state('');
	let allowed_ips = $state('');
	let denied_ips = $state('');
	let blocked_paths = $state('');
	let challenge_suspicious = $state(false);
	let enable_security_headers = $state(true);
	let hsts_max_age = $state(31536000);
	let csp_mode = $state('strict');
	let fail_closed_on_deny = $state(true);

	function parseList(text: string): string[] {
		return text
			.split(/[,\n]/)
			.map((s) => s.trim())
			.filter(Boolean);
	}

	function joinList(items: string[]): string {
		return items.join('\n');
	}

	function applyConfig(cfg: WafPolicyConfig) {
		rate_limit_per_minute = cfg.rate_limit_per_minute;
		login_rate_limit_per_minute = cfg.login_rate_limit_per_minute;
		api_rate_limit_per_minute = cfg.api_rate_limit_per_minute;
		max_body_bytes = cfg.max_body_bytes;
		block_empty_user_agent = cfg.block_empty_user_agent;
		blocked_user_agents = joinList(cfg.blocked_user_agents);
		allowed_ips = joinList(cfg.allowed_ips);
		denied_ips = joinList(cfg.denied_ips);
		blocked_paths = joinList(cfg.blocked_paths);
		challenge_suspicious = cfg.challenge_suspicious;
		enable_security_headers = cfg.enable_security_headers;
		hsts_max_age = cfg.hsts_max_age;
		csp_mode = cfg.csp_mode;
		fail_closed_on_deny = cfg.fail_closed_on_deny;
	}

	function buildConfig(): WafPolicyConfig {
		return {
			rate_limit_per_minute,
			login_rate_limit_per_minute,
			api_rate_limit_per_minute,
			max_body_bytes,
			block_empty_user_agent,
			blocked_user_agents: parseList(blocked_user_agents),
			allowed_ips: parseList(allowed_ips),
			denied_ips: parseList(denied_ips),
			blocked_paths: parseList(blocked_paths),
			challenge_suspicious,
			geo_block_countries: policy?.config.geo_block_countries ?? [],
			enable_security_headers,
			hsts_max_age,
			csp_mode,
			fail_closed_on_deny
		};
	}

	async function load() {
		const [active, history] = await Promise.all([
			api<WafPolicy>('/api/waf/active'),
			api<WafChangeEntry[]>('/api/waf/changes')
		]);
		policy = active;
		changes = history;
		applyConfig(active.config);
	}

	onMount(() => {
		load().catch((e) => (error = e instanceof Error ? e.message : 'Failed to load WAF policy'));
	});

	async function save() {
		if (!policy) return;
		saving = true;
		error = '';
		try {
			const updated = await api<WafPolicy>(`/api/waf/policies/${policy.id}`, {
				method: 'PUT',
				body: JSON.stringify({
					config: buildConfig(),
					reason: reason || null,
					activate: true
				})
			});
			policy = updated;
			applyConfig(updated.config);
			reason = '';
			const history = await api<WafChangeEntry[]>('/api/waf/changes');
			changes = history;
			toast.success('WAF policy saved and activated');
		} catch (e) {
			error = e instanceof Error ? e.message : 'Save failed';
			toast.error(error);
		} finally {
			saving = false;
		}
	}

	async function fetchSnippet() {
		try {
			const res = await api<{ snippet: string }>('/api/waf/caddy-snippet');
			caddySnippet = res.snippet;
			showSnippet = true;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to fetch snippet');
		}
	}
</script>

<PageHeader
	title="WAF"
	description="Web application firewall policy with full change-history tracking"
>
	{#snippet actions()}
		<Button variant="outline" onclick={fetchSnippet}>
			<HugeiconsIcon icon={SourceCodeIcon} class="size-4" strokeWidth={2} />
			Caddy snippet
		</Button>
	{/snippet}
</PageHeader>

{#if error}
	<Alert.Root variant="destructive" class="mb-4">
		<HugeiconsIcon icon={Alert02Icon} class="size-4" strokeWidth={2} />
		<Alert.Description>{error}</Alert.Description>
	</Alert.Root>
{/if}

{#if policy}
	<Card.Root class="mb-6 border-border/60">
		<Card.Header class="flex-row items-start justify-between gap-4">
			<div>
				<Card.Title class="flex items-center gap-2">
					<HugeiconsIcon icon={SecurityLockIcon} class="size-5 text-primary" strokeWidth={2} />
					{policy.name}
				</Card.Title>
				<Card.Description>
					Version {policy.version} · {policy.is_active ? 'Active' : 'Inactive'}
				</Card.Description>
			</div>
			<Badge variant={policy.enabled ? 'default' : 'secondary'}>
				{policy.enabled ? 'Enabled' : 'Disabled'}
			</Badge>
		</Card.Header>
		<Card.Content class="space-y-6">
			<div class="grid gap-4 sm:grid-cols-3">
				<div class="space-y-2">
					<Label>Global rate limit / min</Label>
					<Input type="number" min="1" max="100000" bind:value={rate_limit_per_minute} />
				</div>
				<div class="space-y-2">
					<Label>Login rate limit / min</Label>
					<Input type="number" min="1" max="100000" bind:value={login_rate_limit_per_minute} />
				</div>
				<div class="space-y-2">
					<Label>API rate limit / min</Label>
					<Input type="number" min="1" max="100000" bind:value={api_rate_limit_per_minute} />
				</div>
			</div>

			<div class="grid gap-4 sm:grid-cols-2">
				<div class="space-y-2">
					<Label>Max body bytes</Label>
					<Input type="number" min="1024" max="104857600" bind:value={max_body_bytes} />
				</div>
				<div class="space-y-2">
					<Label>HSTS max age (seconds)</Label>
					<Input type="number" min="0" bind:value={hsts_max_age} />
				</div>
			</div>

			<div class="grid gap-4 sm:grid-cols-2">
				<div class="space-y-2">
					<Label>Allowed IPs <span class="text-muted-foreground">(comma or newline)</span></Label>
					<Textarea bind:value={allowed_ips} rows={3} class="font-mono text-xs" placeholder="203.0.113.0/24" />
				</div>
				<div class="space-y-2">
					<Label>Denied IPs</Label>
					<Textarea bind:value={denied_ips} rows={3} class="font-mono text-xs" placeholder="198.51.100.42" />
				</div>
			</div>

			<div class="grid gap-4 sm:grid-cols-2">
				<div class="space-y-2">
					<Label>Blocked user agents</Label>
					<Textarea bind:value={blocked_user_agents} rows={3} class="font-mono text-xs" placeholder="curl&#10;wget" />
				</div>
				<div class="space-y-2">
					<Label>Blocked paths</Label>
					<Textarea bind:value={blocked_paths} rows={3} class="font-mono text-xs" placeholder="/.env&#10;/wp-admin/*" />
				</div>
			</div>

			<div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
				<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
					<span>Block empty UA</span>
					<Switch bind:checked={block_empty_user_agent} />
				</label>
				<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
					<span>Security headers</span>
					<Switch bind:checked={enable_security_headers} />
				</label>
				<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
					<span>Fail closed on deny</span>
					<Switch bind:checked={fail_closed_on_deny} />
				</label>
				<label class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm">
					<span>Challenge suspicious</span>
					<Switch bind:checked={challenge_suspicious} />
				</label>
			</div>

			<div class="space-y-2 max-w-xs">
				<Label>CSP mode</Label>
				<Select.Root type="single" bind:value={csp_mode}>
					<Select.Trigger class="w-full">{csp_mode}</Select.Trigger>
					<Select.Content>
						<Select.Item value="strict" label="strict">strict</Select.Item>
						<Select.Item value="relaxed" label="relaxed">relaxed</Select.Item>
						<Select.Item value="off" label="off">off</Select.Item>
					</Select.Content>
				</Select.Root>
			</div>

			<div class="space-y-2 border-t border-border/60 pt-5">
				<Label for="reason">Change reason <span class="text-muted-foreground">(recorded in audit log)</span></Label>
				<Input id="reason" bind:value={reason} placeholder="e.g. Tighten login rate limits" />
			</div>
		</Card.Content>
		<Card.Footer class="justify-end gap-2 border-t">
			<Button variant="outline" onclick={() => load()}>
				<HugeiconsIcon icon={RefreshIcon} class="size-4" strokeWidth={2} />
				Reset
			</Button>
			<Button onclick={save} disabled={saving}>
				<HugeiconsIcon icon={SaveEnergy01Icon} class="size-4" strokeWidth={2} />
				{saving ? 'Saving…' : 'Save & activate'}
			</Button>
		</Card.Footer>
	</Card.Root>
{/if}

{#if showSnippet && caddySnippet}
	<Card.Root class="mb-6 border-border/60">
		<Card.Header class="flex-row items-center justify-between">
			<Card.Title>Caddy snippet</Card.Title>
			<Button variant="ghost" size="sm" onclick={() => (showSnippet = false)}>Close</Button>
		</Card.Header>
		<Card.Content>
			<pre class="overflow-x-auto rounded-lg bg-muted/40 p-4 font-mono text-xs">{caddySnippet}</pre>
		</Card.Content>
	</Card.Root>
{/if}

<Card.Root class="border-border/60">
	<Card.Header>
		<Card.Title>Change history</Card.Title>
		<Card.Description>Recent WAF policy modifications</Card.Description>
	</Card.Header>
	<Card.Content>
		{#if changes.length === 0}
			<p class="text-sm text-muted-foreground">No changes recorded yet.</p>
		{:else}
			<Table.Root>
				<Table.Header>
					<Table.Row class="hover:bg-transparent">
						<Table.Head>When</Table.Head>
						<Table.Head>Version</Table.Head>
						<Table.Head>Summary</Table.Head>
						<Table.Head>Actor</Table.Head>
						<Table.Head>Reason</Table.Head>
					</Table.Row>
				</Table.Header>
				<Table.Body>
					{#each changes as entry (entry.id)}
						<Table.Row>
							<Table.Cell class="text-xs whitespace-nowrap">
								{formatRelative(entry.created_at)}
							</Table.Cell>
							<Table.Cell class="font-mono text-xs">
								{entry.version_from ?? '—'} → {entry.version_to}
							</Table.Cell>
							<Table.Cell class="text-xs">{entry.change_summary}</Table.Cell>
							<Table.Cell class="text-xs">{entry.actor_username ?? '—'}</Table.Cell>
							<Table.Cell class="text-xs text-muted-foreground">{entry.reason ?? '—'}</Table.Cell>
						</Table.Row>
					{/each}
				</Table.Body>
			</Table.Root>
		{/if}
	</Card.Content>
</Card.Root>
