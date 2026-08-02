<script lang="ts">
	import { page } from '$app/stores';
	import { cn } from '$lib/utils.js';

	export interface PageSwitcherItem {
		href: string;
		label: string;
	}

	let { items }: { items: PageSwitcherItem[] } = $props();

	const activeHref = $derived.by(() => {
		const pathname = $page.url.pathname;
		let best: string | null = null;
		let bestLen = -1;
		for (const item of items) {
			if (
				pathname === item.href ||
				pathname === `${item.href}/` ||
				pathname.startsWith(`${item.href}/`)
			) {
				if (item.href.length > bestLen) {
					best = item.href;
					bestLen = item.href.length;
				}
			}
		}
		return best;
	});
</script>

<nav class="mb-6 flex flex-wrap gap-1" aria-label="Page sections">
	{#each items as item (item.href)}
		<a
			href={item.href}
			class={cn(
				'inline-flex h-8 items-center rounded-full px-3 text-sm font-medium transition-colors',
				activeHref === item.href
					? 'bg-primary text-primary-foreground'
					: 'text-muted-foreground hover:bg-muted hover:text-foreground'
			)}
			aria-current={activeHref === item.href ? 'page' : undefined}
		>
			{item.label}
		</a>
	{/each}
</nav>
