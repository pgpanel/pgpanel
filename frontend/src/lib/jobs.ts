import { toast } from 'svelte-sonner';
import { getCsrfToken } from './api';

export type Operation = {
	id: string;
	job_type: string;
	status: string;
	progress: number;
	error: string | null;
	result?: unknown;
};

export type OpUpdate = {
	operation: Operation;
	logs: { id: number; level: string; message: string; created_at: string }[];
};

/**
 * Track a background operation until it finishes.
 * Keeps a sonner toast open with live progress + last log line.
 */
export function trackOperation(
	operationId: string,
	opts: {
		title?: string;
		onDone?: (op: Operation, logs: OpUpdate['logs']) => void;
		onFail?: (op: Operation) => void;
	} = {}
): () => void {
	const title = opts.title ?? 'Operation';
	const toastId = `op-${operationId}`;
	let closed = false;

	toast.loading(`${title}…`, {
		id: toastId,
		description: 'Queued',
		duration: Infinity
	});

	// EventSource cannot set CSRF headers; cookie session is enough for GET SSE.
	const es = new EventSource(`/api/operations/${operationId}/events`, {
		withCredentials: true
	});

	const finish = () => {
		if (closed) return;
		closed = true;
		es.close();
	};

	es.addEventListener('update', (ev) => {
		try {
			const data = JSON.parse((ev as MessageEvent).data) as OpUpdate;
			const op = data.operation;
			const lastLog = data.logs?.[data.logs.length - 1]?.message;
			const desc = [
				`${op.status} · ${op.progress ?? 0}%`,
				lastLog ? lastLog.slice(0, 120) : null
			]
				.filter(Boolean)
				.join(' — ');

			if (op.status === 'succeeded') {
				toast.success(`${title} complete`, {
					id: toastId,
					description: desc,
					duration: 6000
				});
				opts.onDone?.(op, data.logs ?? []);
				finish();
			} else if (op.status === 'failed' || op.status === 'cancelled') {
				toast.error(`${title} failed`, {
					id: toastId,
					description: op.error || desc,
					duration: 12000
				});
				opts.onFail?.(op);
				finish();
			} else {
				toast.loading(`${title}…`, {
					id: toastId,
					description: desc,
					duration: Infinity
				});
			}
		} catch {
			/* ignore parse errors */
		}
	});

	es.addEventListener('done', () => finish());
	es.addEventListener('error', () => {
		// Fall back to polling if SSE drops
		if (closed) return;
		pollUntilDone(operationId, toastId, title, opts).finally(finish);
		es.close();
	});

	return finish;
}

async function pollUntilDone(
	operationId: string,
	toastId: string | number,
	title: string,
	opts: {
		onDone?: (op: Operation, logs: OpUpdate['logs']) => void;
		onFail?: (op: Operation) => void;
	}
) {
	for (let i = 0; i < 300; i++) {
		await sleep(2000);
		try {
			const res = await fetch(`/api/operations/${operationId}`, {
				credentials: 'include',
				headers: getCsrfToken() ? { 'x-csrf-token': getCsrfToken() } : {}
			});
			if (!res.ok) continue;
			const op = (await res.json()) as Operation;
			const desc = `${op.status} · ${op.progress ?? 0}%`;
			if (op.status === 'succeeded') {
				toast.success(`${title} complete`, { id: toastId, description: desc, duration: 6000 });
				opts.onDone?.(op, []);
				return;
			}
			if (op.status === 'failed' || op.status === 'cancelled') {
				toast.error(`${title} failed`, {
					id: toastId,
					description: op.error || desc,
					duration: 12000
				});
				opts.onFail?.(op);
				return;
			}
			toast.loading(`${title}…`, { id: toastId, description: desc, duration: Infinity });
		} catch {
			/* retry */
		}
	}
	toast.error(`${title} timed out`, { id: toastId, duration: 8000 });
}

function sleep(ms: number) {
	return new Promise((r) => setTimeout(r, ms));
}
