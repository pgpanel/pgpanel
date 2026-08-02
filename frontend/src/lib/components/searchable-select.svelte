<script lang="ts">
	import * as Popover from '$lib/components/ui/popover/index.js';
	import * as Command from '$lib/components/ui/command/index.js';
	import { buttonVariants } from '$lib/components/ui/button/index.js';
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import { UnfoldMoreIcon, Cancel01Icon } from '@hugeicons/core-free-icons';
	import { cn } from '$lib/utils.js';

	export type SearchableOption = {
		value: string;
		label: string;
		description?: string;
		disabled?: boolean;
		keywords?: string[];
	};

	let {
		value = $bindable(''),
		items = [],
		placeholder = 'Select…',
		searchPlaceholder = 'Search…',
		emptyText = 'No results',
		disabled = false,
		clearable = false,
		loading = false,
		mono = false,
		class: className = '',
		id,
		onValueChange
	}: {
		value?: string;
		items: SearchableOption[];
		placeholder?: string;
		searchPlaceholder?: string;
		emptyText?: string;
		disabled?: boolean;
		clearable?: boolean;
		loading?: boolean;
		mono?: boolean;
		class?: string;
		id?: string;
		onValueChange?: (v: string) => void;
	} = $props();

	let open = $state(false);
	let search = $state('');

	const selectedLabel = $derived(items.find((item) => item.value === value)?.label ?? '');

	function itemKeywords(item: SearchableOption): string[] {
		return [item.label, item.description, ...(item.keywords ?? [])].filter(
			(k): k is string => Boolean(k)
		);
	}

	function filterOption(optionValue: string, query: string, keywords?: string[]): number {
		const q = query.trim().toLowerCase();
		if (!q) return 1;
		const haystack = [optionValue, ...(keywords ?? [])].join(' ').toLowerCase();
		return haystack.includes(q) ? 1 : 0;
	}

	function selectItem(next: string) {
		value = next;
		onValueChange?.(next);
		open = false;
		search = '';
	}

	function clearValue(event: MouseEvent) {
		event.stopPropagation();
		event.preventDefault();
		value = '';
		onValueChange?.('');
	}

	function handleOpenChange(next: boolean) {
		open = next;
		if (!next) search = '';
	}
</script>

<Popover.Root open={open} onOpenChange={handleOpenChange}>
	<div
		class={cn(
			buttonVariants({ variant: 'outline', size: 'default' }),
			'flex w-full items-stretch p-0 font-normal dark:bg-input/30 dark:hover:bg-input/50',
			!selectedLabel && 'text-muted-foreground',
			mono && 'font-mono text-sm',
			className
		)}
	>
		<Popover.Trigger {disabled} class="min-w-0 flex-1">
			{#snippet child({ props })}
				<button
					{...props}
					{id}
					type="button"
					class="flex h-8 w-full items-center justify-between gap-1 rounded-lg bg-transparent px-2.5 text-left outline-none disabled:cursor-not-allowed disabled:opacity-50"
				>
					<span class="truncate">{selectedLabel || placeholder}</span>
					<HugeiconsIcon
						icon={UnfoldMoreIcon}
						class="size-4 shrink-0 text-muted-foreground"
						strokeWidth={2}
					/>
				</button>
			{/snippet}
		</Popover.Trigger>
		{#if clearable && value}
			<button
				type="button"
				class="shrink-0 rounded-r-lg px-1.5 text-muted-foreground hover:bg-muted hover:text-foreground disabled:pointer-events-none disabled:opacity-50"
				{disabled}
				onclick={clearValue}
				aria-label="Clear selection"
			>
				<HugeiconsIcon icon={Cancel01Icon} class="size-3.5" strokeWidth={2} />
			</button>
		{/if}
	</div>
	<Popover.Content class="w-(--bits-popover-anchor-width) p-0" align="start">
		<Command.Root filter={filterOption}>
			<Command.Input placeholder={searchPlaceholder} bind:value={search} />
			<Command.List class="max-h-60">
				{#if loading}
					<Command.Loading>Loading…</Command.Loading>
				{:else}
					{#each items as item (item.value)}
						<Command.Item
							value={item.value}
							keywords={itemKeywords(item)}
							disabled={item.disabled}
							onSelect={() => selectItem(item.value)}
						>
							<div class="flex min-w-0 flex-col gap-0.5">
								<span class={cn('truncate', mono && 'font-mono text-xs')}>{item.label}</span>
								{#if item.description}
									<span class="truncate text-xs text-muted-foreground">{item.description}</span>
								{/if}
							</div>
						</Command.Item>
					{/each}
					<Command.Empty>{emptyText}</Command.Empty>
				{/if}
			</Command.List>
		</Command.Root>
	</Popover.Content>
</Popover.Root>
