<script lang="ts">
	import * as Dialog from '$lib/components/ui/dialog/index.js';
	import { Button } from '$lib/components/ui/button/index.js';

	let {
		open = $bindable(false),
		title,
		description = '',
		confirmLabel = 'Confirm',
		cancelLabel = 'Cancel',
		variant = 'default',
		loading = false,
		onConfirm
	}: {
		open?: boolean;
		title: string;
		description?: string;
		confirmLabel?: string;
		cancelLabel?: string;
		variant?: 'default' | 'destructive';
		loading?: boolean;
		onConfirm: () => void | Promise<void>;
	} = $props();

	let confirming = $state(false);

	async function handleConfirm() {
		confirming = true;
		try {
			await onConfirm();
			open = false;
		} finally {
			confirming = false;
		}
	}
</script>

<Dialog.Root bind:open>
	<Dialog.Content class="sm:max-w-md">
		<Dialog.Header>
			<Dialog.Title>{title}</Dialog.Title>
			{#if description}
				<Dialog.Description>{description}</Dialog.Description>
			{/if}
		</Dialog.Header>
		<Dialog.Footer class="gap-2 border-t-0 bg-transparent sm:justify-end">
			<Button variant="outline" onclick={() => (open = false)} disabled={confirming || loading}>
				{cancelLabel}
			</Button>
			<Button
				variant={variant === 'destructive' ? 'destructive' : 'default'}
				onclick={handleConfirm}
				disabled={confirming || loading}
			>
				{confirming || loading ? 'Working…' : confirmLabel}
			</Button>
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>
