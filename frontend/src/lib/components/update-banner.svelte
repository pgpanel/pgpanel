<script lang="ts">
	import { goto } from '$app/navigation';
	import { updateStatus, checkForUpdates, updateChecking } from '$lib/updates';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Badge } from '$lib/components/ui/badge/index.js';
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import { Download04Icon, RefreshIcon } from '@hugeicons/core-free-icons';

	let { compact = false }: { compact?: boolean } = $props();
</script>

{#if $updateStatus?.update_available}
	{#if compact}
		<Badge variant="secondary" class="bg-amber-500/15 text-amber-300 ring-1 ring-amber-500/30">
			Update
		</Badge>
	{:else}
		<div
			class="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-amber-500/25 bg-amber-500/8 px-4 py-2.5 text-sm"
		>
			<div class="flex items-center gap-2">
				<HugeiconsIcon icon={Download04Icon} class="size-4 text-amber-400" strokeWidth={2} />
				<span>
					Update available:
					<span class="font-mono text-xs">{$updateStatus.current_version}</span>
					→
					<span class="font-mono text-xs text-amber-300">{$updateStatus.latest_version}</span>
				</span>
			</div>
			<div class="flex items-center gap-2">
				<Button variant="ghost" size="sm" onclick={() => checkForUpdates()} disabled={$updateChecking}>
					<HugeiconsIcon icon={RefreshIcon} class="size-3.5" strokeWidth={2} />
					{$updateChecking ? 'Checking…' : 'Recheck'}
				</Button>
				<Button
					size="sm"
					onclick={() => goto('/settings#updates')}
				>
					Update in Settings
				</Button>
			</div>
		</div>
	{/if}
{/if}
