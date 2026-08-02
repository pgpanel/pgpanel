<script lang="ts">
	import { Badge } from '$lib/components/ui/badge/index.js';
	import { statusVariant } from '$lib/api';

	let {
		status,
		class: className = ''
	}: { status: string | null | undefined; class?: string } = $props();

	const label = $derived((status ?? 'unknown').replaceAll('_', ' '));
</script>

<Badge variant={statusVariant(status)} class="font-mono text-[11px] capitalize {className}">
	{label}
</Badge>
