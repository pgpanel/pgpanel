<script lang="ts">
	import { page } from '$app/stores';
	import { goto } from '$app/navigation';
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import {
		DashboardSquare01Icon,
		DatabaseIcon,
		PlusSignIcon,
		Activity01Icon,
		Shield01Icon,
		Settings01Icon,
		Logout03Icon,
		ServerStack01Icon,
		CloudServerIcon,
		SecurityLockIcon,
		UserGroupIcon,
		CloudUploadIcon,
		DatabaseSyncIcon,
		Chart01Icon,
		Notification01Icon
	} from '@hugeicons/core-free-icons';
	import * as Sidebar from '$lib/components/ui/sidebar/index.js';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Badge } from '$lib/components/ui/badge/index.js';
	import UpdateBanner from '$lib/components/update-banner.svelte';
	import { updateStatus } from '$lib/updates';
	import { logout, user } from '$lib/auth';

	const NEW_CLUSTER_HREF = '/clusters/new';

	const navGroups = [
		{
			label: 'Overview',
			items: [
				{ href: '/dashboard', label: 'Dashboard', icon: DashboardSquare01Icon },
				{ href: '/monitoring', label: 'Monitoring', icon: Chart01Icon },
				{ href: '/alerts', label: 'Alerts', icon: Notification01Icon }
			]
		},
		{
			label: 'Infrastructure',
			items: [
				{ href: '/clusters', label: 'Clusters', icon: DatabaseIcon },
				{ href: '/clusters/new', label: 'New cluster', icon: PlusSignIcon },
				{ href: '/nodes', label: 'Nodes', icon: CloudServerIcon },
				{ href: '/replicas', label: 'Replicas', icon: DatabaseSyncIcon },
				{ href: '/destinations', label: 'Destinations', icon: CloudUploadIcon }
			]
		},
		{
			label: 'Security',
			items: [
				{ href: '/waf', label: 'WAF', icon: SecurityLockIcon },
				{ href: '/users', label: 'Users', icon: UserGroupIcon },
				{ href: '/audit', label: 'Audit log', icon: Shield01Icon }
			]
		},
		{
			label: 'System',
			items: [
				{ href: '/operations', label: 'Operations', icon: Activity01Icon },
				{ href: '/settings', label: 'Settings', icon: Settings01Icon, showUpdateBadge: true }
			]
		}
	] as const;

	function isActive(href: string, path: string) {
		if (href === '/clusters') return path === '/clusters' || path.startsWith('/clusters/');
		if (href === '/clusters/new') return path === '/clusters/new';
		return path === href || path.startsWith(href + '/');
	}

	function navItemActive(href: string, path: string) {
		if (href === '/clusters') return path === '/clusters';
		return isActive(href, path);
	}

	async function handleLogout() {
		await logout();
		goto('/login');
	}
</script>

<Sidebar.Root collapsible="icon" variant="inset" class="pg-sidebar">
	<Sidebar.Header class="pg-sidebar-header">
		<a
			href="/dashboard"
			class="pg-brand"
			aria-label="PgPanel home"
		>
			<div class="pg-brand-mark">
				<HugeiconsIcon icon={ServerStack01Icon} class="size-4" strokeWidth={2} />
			</div>
			<div class="pg-brand-text group-data-[collapsible=icon]:hidden">
				<span class="pg-brand-name">PgPanel</span>
				<span class="pg-brand-meta">
					{#if $updateStatus?.current_version}
						<span class="pg-brand-version">v{$updateStatus.current_version}</span>
					{/if}
					<span class="pg-status">
						<span class="pg-status-dot" aria-hidden="true"></span>
						<span class="pg-status-label">Control plane</span>
					</span>
				</span>
			</div>
		</a>
	</Sidebar.Header>

	<Sidebar.Content class="pg-sidebar-content">
		<Sidebar.Group class="pg-cta-group">
			<Sidebar.Menu>
				<Sidebar.MenuItem>
					<Sidebar.MenuButton
						isActive={$page.url.pathname === NEW_CLUSTER_HREF}
						tooltipContent="New cluster"
						class="pg-new-cluster-cta"
					>
						{#snippet child({ props })}
							<a
								href={NEW_CLUSTER_HREF}
								{...props}
								aria-current={$page.url.pathname === NEW_CLUSTER_HREF ? 'page' : undefined}
							>
								<HugeiconsIcon icon={PlusSignIcon} strokeWidth={2} />
								<span>New cluster</span>
							</a>
						{/snippet}
					</Sidebar.MenuButton>
				</Sidebar.MenuItem>
			</Sidebar.Menu>
		</Sidebar.Group>

		{#each navGroups as group (group.label)}
			<Sidebar.Group class="pg-nav-group">
				<Sidebar.GroupLabel class="pg-group-label">{group.label}</Sidebar.GroupLabel>
				<Sidebar.GroupContent>
					<Sidebar.Menu>
						{#each group.items.filter((item) => item.href !== NEW_CLUSTER_HREF) as item (item.href)}
							{@const active = navItemActive(item.href, $page.url.pathname)}
							<Sidebar.MenuItem>
								<Sidebar.MenuButton
									isActive={active}
									tooltipContent={item.label}
									class="pg-nav-item"
								>
									{#snippet child({ props })}
										<a
											href={item.href}
											{...props}
											class="pg-nav-link"
											aria-current={active ? 'page' : undefined}
										>
											<HugeiconsIcon icon={item.icon} strokeWidth={2} />
											<span>{item.label}</span>
											{#if 'showUpdateBadge' in item && item.showUpdateBadge && $updateStatus?.update_available}
												<Badge class="pg-update-badge ms-auto">
													<span class="group-data-[collapsible=icon]:sr-only">Update</span>
												</Badge>
											{/if}
										</a>
									{/snippet}
								</Sidebar.MenuButton>
							</Sidebar.MenuItem>
						{/each}
					</Sidebar.Menu>
				</Sidebar.GroupContent>
			</Sidebar.Group>
		{/each}
	</Sidebar.Content>

	<Sidebar.Footer class="pg-sidebar-footer">
		<div class="group-data-[collapsible=icon]:hidden">
			<UpdateBanner compact />
		</div>
		<div class="pg-account group-data-[collapsible=icon]:justify-center">
			<div class="pg-account-avatar" aria-hidden="true">
				{$user?.username?.slice(0, 1).toUpperCase() ?? '?'}
			</div>
			<div class="pg-account-info group-data-[collapsible=icon]:hidden">
				<p class="pg-account-name">{$user?.username}</p>
				<p class="pg-account-role capitalize">{$user?.role ?? '—'}</p>
			</div>
		</div>
		<Button
			variant="ghost"
			size="sm"
			class="pg-logout-btn w-full justify-start gap-2 group-data-[collapsible=icon]:size-8 group-data-[collapsible=icon]:justify-center group-data-[collapsible=icon]:p-0"
			onclick={handleLogout}
		>
			<HugeiconsIcon icon={Logout03Icon} class="size-4" strokeWidth={2} />
			<span class="group-data-[collapsible=icon]:hidden">Log out</span>
		</Button>
	</Sidebar.Footer>
	<Sidebar.Rail />
</Sidebar.Root>

<style>
	.pg-sidebar-header {
		padding: 0.75rem 0.625rem 0.5rem;
	}

	.pg-brand {
		display: flex;
		align-items: center;
		gap: 0.625rem;
		padding: 0.125rem 0.25rem;
		border-radius: 0.5rem;
		outline: none;
		text-decoration: none;
	}

	.pg-brand:focus-visible {
		box-shadow: 0 0 0 2px var(--sidebar-ring);
	}

	.pg-brand-mark {
		display: flex;
		width: 2rem;
		height: 2rem;
		flex-shrink: 0;
		align-items: center;
		justify-content: center;
		border-radius: 0.5rem;
		border: 1px solid oklch(0.72 0.17 155 / 22%);
		background: oklch(0.72 0.17 155 / 10%);
		color: oklch(0.78 0.14 155);
	}

	.pg-brand-text {
		display: grid;
		min-width: 0;
		flex: 1;
		gap: 0.125rem;
		text-align: left;
		line-height: 1.25;
	}

	.pg-brand-name {
		font-size: 0.875rem;
		font-weight: 600;
		letter-spacing: -0.02em;
		color: var(--sidebar-foreground);
	}

	.pg-brand-meta {
		display: flex;
		align-items: center;
		gap: 0.375rem;
		font-size: 0.6875rem;
		color: oklch(0.58 0.02 160);
	}

	.pg-brand-version {
		font-family: var(--font-mono);
		font-size: 0.625rem;
		color: oklch(0.62 0.04 155);
	}

	.pg-status {
		display: flex;
		align-items: center;
		gap: 0.25rem;
	}

	.pg-status-dot {
		width: 0.375rem;
		height: 0.375rem;
		border-radius: 9999px;
		background: oklch(0.72 0.17 155);
		box-shadow: 0 0 6px oklch(0.72 0.17 155 / 45%);
	}

	.pg-status-label {
		letter-spacing: 0.01em;
	}

	:global(.pg-sidebar-content) {
		gap: 0.125rem;
		padding-top: 0.125rem;
	}

	:global(.pg-cta-group) {
		padding: 0 0.5rem 0.375rem;
	}

	:global(.pg-nav-group) {
		padding: 0.125rem 0.5rem;
	}

	:global(.pg-group-label) {
		height: auto;
		margin-bottom: 0.125rem;
		padding: 0.25rem 0.5rem 0.125rem;
		font-size: 0.625rem;
		font-weight: 500;
		letter-spacing: 0.08em;
		text-transform: uppercase;
		color: oklch(0.5 0.02 160);
		opacity: 0.85;
	}

	:global(.pg-new-cluster-cta) {
		height: 2.25rem;
		border-radius: 0.5rem;
		border: 1px solid oklch(0.72 0.17 155 / 28%);
		background: oklch(0.72 0.17 155 / 12%);
		color: oklch(0.82 0.12 155);
		font-weight: 500;
		transition:
			background-color 0.15s ease,
			border-color 0.15s ease,
			color 0.15s ease;
	}

	:global(.pg-new-cluster-cta:hover) {
		background: oklch(0.72 0.17 155 / 18%);
		border-color: oklch(0.72 0.17 155 / 38%);
		color: oklch(0.9 0.1 155);
	}

	:global(.pg-new-cluster-cta[data-active='true']) {
		background: oklch(0.72 0.17 155);
		border-color: oklch(0.72 0.17 155);
		color: oklch(0.16 0.03 155);
	}

	:global(.pg-nav-item) {
		height: 2rem;
		border-radius: 0.4375rem;
		color: oklch(0.68 0.02 160);
		font-weight: 400;
		transition:
			background-color 0.15s ease,
			color 0.15s ease;
	}

	:global(.pg-nav-item:hover) {
		background: oklch(1 0 0 / 4%);
		color: oklch(0.88 0.01 160);
	}

	:global(.pg-nav-item[data-active='true']) {
		background: oklch(0.72 0.17 155 / 14%);
		color: oklch(0.86 0.11 155);
		font-weight: 500;
	}

	:global(.pg-nav-link) {
		position: relative;
	}

	:global(.pg-nav-item[data-active='true'] .pg-nav-link)::before {
		content: '';
		position: absolute;
		left: -0.5rem;
		top: 50%;
		width: 2px;
		height: 0.875rem;
		border-radius: 1px;
		background: oklch(0.72 0.17 155);
		transform: translateY(-50%);
	}

	:global(.pg-update-badge) {
		background: oklch(0.75 0.14 85 / 18%);
		padding: 0 0.375rem;
		font-size: 0.625rem;
		color: oklch(0.82 0.1 85);
		border: 1px solid oklch(0.75 0.14 85 / 28%);
	}

	:global(.group-data-\[collapsible\=icon\] .pg-update-badge) {
		position: absolute;
		top: 0;
		right: 0;
		width: 0.5rem;
		height: 0.5rem;
		padding: 0;
		border-radius: 9999px;
		border: none;
		background: oklch(0.75 0.14 85);
	}

	.pg-sidebar-footer {
		gap: 0.5rem;
		padding: 0.625rem;
		border-top: 1px solid oklch(1 0 0 / 6%);
	}

	.pg-account {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		padding: 0.375rem 0.25rem;
	}

	.pg-account-avatar {
		display: flex;
		width: 2rem;
		height: 2rem;
		flex-shrink: 0;
		align-items: center;
		justify-content: center;
		border-radius: 0.4375rem;
		border: 1px solid oklch(1 0 0 / 8%);
		background: oklch(1 0 0 / 4%);
		font-size: 0.75rem;
		font-weight: 600;
		color: oklch(0.78 0.1 155);
	}

	.pg-account-info {
		min-width: 0;
		flex: 1;
	}

	.pg-account-name {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-size: 0.8125rem;
		font-weight: 500;
		line-height: 1.3;
		color: var(--sidebar-foreground);
	}

	.pg-account-role {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-size: 0.6875rem;
		line-height: 1.3;
		color: oklch(0.55 0.02 160);
	}

	:global(.pg-logout-btn) {
		color: oklch(0.62 0.02 160);
	}

	:global(.pg-logout-btn:hover) {
		background: oklch(1 0 0 / 5%);
		color: oklch(0.88 0.01 160);
	}

	@media (prefers-reduced-motion: reduce) {
		:global(.pg-new-cluster-cta),
		:global(.pg-nav-item) {
			transition: none;
		}
	}
</style>
