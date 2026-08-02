<script lang="ts">
	import { page } from '$app/stores';
	import { goto } from '$app/navigation';
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import {
		DashboardSquare01Icon,
		DatabaseIcon,
		PlusSignIcon,
		Shield01Icon,
		Settings01Icon,
		Logout03Icon,
		ServerStack01Icon,
		CloudServerIcon,
		SecurityLockIcon,
		UserGroupIcon,
		CloudUploadIcon,
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
				{ href: '/nodes', label: 'Fleet', icon: CloudServerIcon },
				{ href: '/destinations', label: 'Backups', icon: CloudUploadIcon }
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
				{ href: '/settings', label: 'Settings', icon: Settings01Icon, showUpdateBadge: true }
			]
		}
	] as const;

	function navItemActive(href: string, path: string) {
		if (href === '/clusters') {
			if (path === '/clusters') return true;
			if (path === '/clusters/new' || path.startsWith('/clusters/new/')) return false;
			return path.startsWith('/clusters/');
		}
		return path === href || path.startsWith(href + '/');
	}

	async function handleLogout() {
		await logout();
		goto('/login');
	}
</script>

<Sidebar.Root collapsible="icon" variant="inset">
	<Sidebar.Header class="border-b border-sidebar-border px-3 py-3">
		<a
			href="/dashboard"
			class="flex items-center gap-2.5 rounded-md outline-none focus-visible:ring-2 focus-visible:ring-sidebar-ring group-data-[collapsible=icon]:justify-center"
			aria-label="PgPanel home"
		>
			<div class="pg-brand-mark">
				<HugeiconsIcon icon={ServerStack01Icon} class="size-4" strokeWidth={2} />
			</div>
			<div class="min-w-0 group-data-[collapsible=icon]:hidden">
				<p class="truncate text-sm font-semibold leading-tight text-sidebar-foreground">PgPanel</p>
				<p class="truncate text-[11px] leading-tight text-sidebar-foreground/50">
					{#if $updateStatus?.current_version}
						v{$updateStatus.current_version}
					{:else}
						Control plane
					{/if}
				</p>
			</div>
		</a>
	</Sidebar.Header>

	<Sidebar.Content class="gap-1 px-2 py-2">
		<Sidebar.Group class="p-0">
			<Sidebar.Menu>
				<Sidebar.MenuItem>
					<Sidebar.MenuButton
						isActive={$page.url.pathname === NEW_CLUSTER_HREF}
						tooltipContent="New cluster"
						class="h-9 border border-sidebar-primary/30 bg-sidebar-primary/10 font-medium text-sidebar-primary hover:bg-sidebar-primary/15 data-[active=true]:border-sidebar-primary data-[active=true]:bg-sidebar-primary data-[active=true]:text-sidebar-primary-foreground"
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
			<Sidebar.Group class="p-0">
				<Sidebar.GroupLabel
					class="h-7 px-2 text-[10px] font-medium uppercase tracking-wider text-sidebar-foreground/45"
				>
					{group.label}
				</Sidebar.GroupLabel>
				<Sidebar.GroupContent>
					<Sidebar.Menu>
						{#each group.items as item (item.href)}
							{@const active = navItemActive(item.href, $page.url.pathname)}
							<Sidebar.MenuItem>
								<Sidebar.MenuButton
									isActive={active}
									tooltipContent={item.label}
									class="h-8 text-sidebar-foreground/65 hover:text-sidebar-foreground data-[active=true]:bg-sidebar-accent data-[active=true]:text-sidebar-accent-foreground"
								>
									{#snippet child({ props })}
										<a
											href={item.href}
											{...props}
											aria-current={active ? 'page' : undefined}
										>
											<HugeiconsIcon icon={item.icon} strokeWidth={2} />
											<span>{item.label}</span>
											{#if 'showUpdateBadge' in item && item.showUpdateBadge && $updateStatus?.update_available}
												<Badge
													variant="outline"
													class="ms-auto border-chart-3/30 bg-chart-3/10 px-1.5 text-[10px] text-chart-3 group-data-[collapsible=icon]:absolute group-data-[collapsible=icon]:top-0.5 group-data-[collapsible=icon]:right-0.5 group-data-[collapsible=icon]:size-2 group-data-[collapsible=icon]:border-0 group-data-[collapsible=icon]:bg-chart-3 group-data-[collapsible=icon]:p-0"
												>
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

	<Sidebar.Footer class="gap-2.5 border-t border-sidebar-border p-3">
		<div class="group-data-[collapsible=icon]:hidden">
			<UpdateBanner compact />
		</div>

		<div
			class="flex items-center gap-2.5 px-1 group-data-[collapsible=icon]:justify-center group-data-[collapsible=icon]:px-0"
		>
			<div
				class="flex size-8 shrink-0 items-center justify-center rounded-md border border-sidebar-border bg-sidebar-accent/40 text-xs font-semibold text-sidebar-primary"
				aria-hidden="true"
			>
				{$user?.username?.slice(0, 1).toUpperCase() ?? '?'}
			</div>
			<div class="min-w-0 flex-1 group-data-[collapsible=icon]:hidden">
				<p class="truncate text-sm font-medium leading-tight text-sidebar-foreground">
					{$user?.username}
				</p>
				<p class="truncate text-xs capitalize leading-tight text-sidebar-foreground/50">
					{$user?.role ?? '—'}
				</p>
			</div>
		</div>

		<Button
			variant="ghost"
			size="sm"
			class="h-8 w-full justify-start gap-2 text-sidebar-foreground/60 hover:bg-sidebar-accent/50 hover:text-sidebar-foreground group-data-[collapsible=icon]:size-8 group-data-[collapsible=icon]:justify-center group-data-[collapsible=icon]:p-0"
			onclick={handleLogout}
		>
			<HugeiconsIcon icon={Logout03Icon} class="size-4" strokeWidth={2} />
			<span class="group-data-[collapsible=icon]:hidden">Log out</span>
		</Button>
	</Sidebar.Footer>

	<Sidebar.Rail />
</Sidebar.Root>

<style>
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
</style>
