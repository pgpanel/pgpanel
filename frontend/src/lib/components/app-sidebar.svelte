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
	import { Separator } from '$lib/components/ui/separator/index.js';
	import { logout, user } from '$lib/auth';

	const nav = [
		{ href: '/dashboard', label: 'Dashboard', icon: DashboardSquare01Icon },
		{ href: '/clusters', label: 'Clusters', icon: DatabaseIcon },
		{ href: '/clusters/new', label: 'New cluster', icon: PlusSignIcon },
		{ href: '/nodes', label: 'Nodes', icon: CloudServerIcon },
		{ href: '/replicas', label: 'Replicas', icon: DatabaseSyncIcon },
		{ href: '/destinations', label: 'Destinations', icon: CloudUploadIcon },
		{ href: '/monitoring', label: 'Monitoring', icon: Chart01Icon },
		{ href: '/alerts', label: 'Alerts', icon: Notification01Icon },
		{ href: '/waf', label: 'WAF', icon: SecurityLockIcon },
		{ href: '/operations', label: 'Operations', icon: Activity01Icon },
		{ href: '/users', label: 'Users', icon: UserGroupIcon },
		{ href: '/audit', label: 'Audit log', icon: Shield01Icon },
		{ href: '/settings', label: 'Settings', icon: Settings01Icon }
	] as const;

	function isActive(href: string, path: string) {
		if (href === '/clusters') return path === '/clusters' || path.startsWith('/clusters/');
		if (href === '/clusters/new') return path === '/clusters/new';
		return path === href || path.startsWith(href + '/');
	}

	async function handleLogout() {
		await logout();
		goto('/login');
	}
</script>

<Sidebar.Root collapsible="icon" variant="inset">
	<Sidebar.Header class="gap-3 px-3 py-4">
		<a href="/dashboard" class="flex items-center gap-2.5 rounded-lg px-1 outline-none ring-sidebar-ring focus-visible:ring-2">
			<div
				class="flex size-8 shrink-0 items-center justify-center rounded-lg bg-primary text-primary-foreground shadow-sm shadow-primary/30"
			>
				<HugeiconsIcon icon={ServerStack01Icon} class="size-4" strokeWidth={2} />
			</div>
			<div class="grid min-w-0 flex-1 text-left text-sm leading-tight group-data-[collapsible=icon]:hidden">
				<span class="truncate font-semibold tracking-tight">PgPanel</span>
				<span class="truncate text-xs text-muted-foreground">PostgreSQL control plane</span>
			</div>
		</a>
	</Sidebar.Header>

	<Sidebar.Content>
		<Sidebar.Group>
			<Sidebar.GroupLabel>Manage</Sidebar.GroupLabel>
			<Sidebar.GroupContent>
				<Sidebar.Menu>
					{#each nav as item}
						{@const active =
							item.href === '/clusters'
								? $page.url.pathname === '/clusters'
								: isActive(item.href, $page.url.pathname)}
						<Sidebar.MenuItem>
							<Sidebar.MenuButton isActive={active} tooltipContent={item.label}>
								{#snippet child({ props })}
									<a href={item.href} {...props}>
										<HugeiconsIcon icon={item.icon} strokeWidth={2} />
										<span>{item.label}</span>
									</a>
								{/snippet}
							</Sidebar.MenuButton>
						</Sidebar.MenuItem>
					{/each}
				</Sidebar.Menu>
			</Sidebar.GroupContent>
		</Sidebar.Group>
	</Sidebar.Content>

	<Sidebar.Footer class="gap-2 p-3">
		<Separator class="mb-1" />
		<div class="flex items-center gap-2 rounded-lg bg-sidebar-accent/40 px-2 py-2 group-data-[collapsible=icon]:justify-center">
			<div
				class="flex size-8 shrink-0 items-center justify-center rounded-full bg-primary/15 text-xs font-semibold text-primary"
			>
				{$user?.username?.slice(0, 1).toUpperCase() ?? '?'}
			</div>
			<div class="min-w-0 flex-1 group-data-[collapsible=icon]:hidden">
				<p class="truncate text-sm font-medium">{$user?.username}</p>
				<p class="truncate text-xs text-muted-foreground capitalize">{$user?.role ?? '—'}</p>
			</div>
		</div>
		<Button
			variant="outline"
			size="sm"
			class="w-full justify-start gap-2 group-data-[collapsible=icon]:size-8 group-data-[collapsible=icon]:justify-center group-data-[collapsible=icon]:p-0"
			onclick={handleLogout}
		>
			<HugeiconsIcon icon={Logout03Icon} class="size-4" strokeWidth={2} />
			<span class="group-data-[collapsible=icon]:hidden">Log out</span>
		</Button>
	</Sidebar.Footer>
	<Sidebar.Rail />
</Sidebar.Root>
