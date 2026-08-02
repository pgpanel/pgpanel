<script lang="ts">
	import '../app.css';
	import { onMount } from 'svelte';
	import { page } from '$app/stores';
	import { goto } from '$app/navigation';
	import { authReady, refreshMe, user } from '$lib/auth';
	import { api } from '$lib/api';
	import { isWizardMarkedDone, markWizardDone, wizardNeeded as wizardNeededStore } from '$lib/wizard';
	import * as Sidebar from '$lib/components/ui/sidebar/index.js';
	import { Separator } from '$lib/components/ui/separator/index.js';
	import * as Breadcrumb from '$lib/components/ui/breadcrumb/index.js';
	import { Toaster } from '$lib/components/ui/sonner/index.js';
	import { Skeleton } from '$lib/components/ui/skeleton/index.js';
	import AppSidebar from '$lib/components/app-sidebar.svelte';
	import UpdateBanner from '$lib/components/update-banner.svelte';
	import { fetchUpdateStatus } from '$lib/updates';

	let { children } = $props();

	const publicPaths = ['/login', '/setup'];

	onMount(async () => {
		await refreshMe();
		fetchUpdateStatus();
	});

	async function checkWizard() {
		if (isWizardMarkedDone()) {
			wizardNeededStore.set(false);
			return false;
		}
		const cached = $wizardNeededStore;
		if (cached !== null) return cached;
		try {
			const s = await api<{ wizard_completed: boolean }>('/api/settings/wizard');
			const need = !s.wizard_completed;
			wizardNeededStore.set(need);
			if (!need) markWizardDone();
			return need;
		} catch {
			wizardNeededStore.set(false);
			return false;
		}
	}

	$effect(() => {
		if (!$authReady) return;
		const path = $page.url.pathname;
		if (!$user && !publicPaths.includes(path)) {
			goto('/login');
			return;
		}
		if ($user && (path === '/login' || path === '/setup')) {
			checkWizard().then((need) => {
				goto(need ? '/wizard' : '/dashboard');
			});
			return;
		}
		if ($user && path !== '/wizard' && !publicPaths.includes(path)) {
			checkWizard().then((need) => {
				if (need) goto('/wizard');
			});
		}
	});

	const crumb = $derived.by(() => {
		const parts = $page.url.pathname.split('/').filter(Boolean);
		if (parts.length === 0) return [{ label: 'Home', href: '/dashboard' }];
		const labels: Record<string, string> = {
			dashboard: 'Dashboard',
			clusters: 'Clusters',
			new: 'New',
			operations: 'Operations',
			audit: 'Audit',
			settings: 'Settings',
			databases: 'Databases',
			browser: 'Browser',
			query: 'SQL console',
			backup: 'Backup',
			timetravel: 'Time travel',
			wizard: 'Setup wizard',
			nodes: 'Fleet',
			monitoring: 'Monitoring',
			replicas: 'Replicas',
			destinations: 'Backups',
			join: 'Join',
			alerts: 'Alerts',
			waf: 'WAF',
			users: 'Users'
		};
		const out: { label: string; href: string }[] = [];
		let acc = '';
		for (const p of parts) {
			acc += `/${p}`;
			out.push({ label: labels[p] ?? (p.length > 12 ? p.slice(0, 8) + '…' : p), href: acc });
		}
		return out;
	});

	const bare = $derived(
		publicPaths.includes($page.url.pathname) || $page.url.pathname === '/wizard'
	);
</script>

<Toaster richColors position="top-right" theme="dark" closeButton />

{#if !$authReady}
	<div class="flex min-h-screen items-center justify-center bg-background">
		<div class="flex w-64 flex-col gap-3">
			<Skeleton class="h-8 w-32" />
			<Skeleton class="h-4 w-full" />
			<Skeleton class="h-4 w-3/4" />
		</div>
	</div>
{:else if bare}
	{@render children()}
{:else if $user}
	<Sidebar.Provider>
		<AppSidebar />
		<Sidebar.Inset class="shell-inset">
			<header
				class="sticky top-0 z-20 flex h-14 shrink-0 items-center gap-2 border-b border-border/60 bg-background/85 px-4 backdrop-blur-md supports-[backdrop-filter]:bg-background/70"
			>
				<Sidebar.Trigger class="-ms-1" />
				<Separator orientation="vertical" class="mx-1 data-[orientation=vertical]:h-4" />
				<Breadcrumb.Root class="min-w-0 flex-1">
					<Breadcrumb.List>
						{#each crumb as c, i}
							<Breadcrumb.Item>
								{#if i === crumb.length - 1}
									<Breadcrumb.Page>{c.label}</Breadcrumb.Page>
								{:else}
									<Breadcrumb.Link href={c.href}>{c.label}</Breadcrumb.Link>
								{/if}
							</Breadcrumb.Item>
							{#if i < crumb.length - 1}
								<Breadcrumb.Separator />
							{/if}
						{/each}
					</Breadcrumb.List>
				</Breadcrumb.Root>
			</header>
			<div class="flex flex-1 flex-col gap-4 p-4 md:p-6 lg:p-8">
				<UpdateBanner />
				<main class="page-frame flex-1">
					{@render children()}
				</main>
			</div>
		</Sidebar.Inset>
	</Sidebar.Provider>
{/if}
