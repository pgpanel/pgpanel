<script lang="ts">
	import { Badge } from '$lib/components/ui/badge/index.js';
	import { statusVariant } from '$lib/api';

	let { status, class: className = '' }: { status: string; class?: string } = $props();
</script>

<Badge variant={statusVariant(status)} class="font-mono text-[11px] capitalize {className}">
	{status.replaceAll('_', ' ')}
</Badge>
