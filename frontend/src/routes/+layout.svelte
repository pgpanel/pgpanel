<script lang="ts">
	import '../app.css';
	import { onMount } from 'svelte';
	import { page } from '$app/stores';
	import { goto } from '$app/navigation';
	import { authReady, refreshMe, user } from '$lib/auth';
	import { api } from '$lib/api';
	import * as Sidebar from '$lib/components/ui/sidebar/index.js';
	import { Separator } from '$lib/components/ui/separator/index.js';
	import * as Breadcrumb from '$lib/components/ui/breadcrumb/index.js';
	import { Toaster } from '$lib/components/ui/sonner/index.js';
	import { Skeleton } from '$lib/components/ui/skeleton/index.js';
	import AppSidebar from '$lib/components/app-sidebar.svelte';

	let { children } = $props();

	const publicPaths = ['/login', '/setup'];
	let wizardChecked = $state(false);
	let wizardNeeded = $state(false);

	onMount(async () => {
		await refreshMe();
	});

	async function checkWizard() {
		if (wizardChecked) return wizardNeeded;
		if (typeof sessionStorage !== 'undefined' && sessionStorage.getItem('pgpanel_wizard_done') === '1') {
			wizardChecked = true;
			wizardNeeded = false;
			return false;
		}
		wizardChecked = true;
		try {
			const s = await api<{ wizard_completed: boolean }>('/api/settings/wizard');
			wizardNeeded = !s.wizard_completed;
		} catch {
			wizardNeeded = false;
		}
		return wizardNeeded;
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
			wizard: 'Setup wizard'
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
		<Sidebar.Inset>
			<header
				class="sticky top-0 z-20 flex h-14 shrink-0 items-center gap-2 border-b bg-background/80 px-4 backdrop-blur supports-[backdrop-filter]:bg-background/60"
			>
				<Sidebar.Trigger class="-ms-1" />
				<Separator orientation="vertical" class="mx-1 data-[orientation=vertical]:h-4" />
				<Breadcrumb.Root>
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
				{@render children()}
			</div>
		</Sidebar.Inset>
	</Sidebar.Provider>
{/if}
