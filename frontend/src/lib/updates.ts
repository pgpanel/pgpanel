import { writable } from 'svelte/store';
import { api, type UpdateStatus } from './api';

export const updateStatus = writable<UpdateStatus | null>(null);
export const updateChecking = writable(false);
export const updateError = writable<string | null>(null);

export async function fetchUpdateStatus(): Promise<UpdateStatus | null> {
	updateError.set(null);
	try {
		const status = await api<UpdateStatus>('/api/updates/status');
		updateStatus.set(status);
		return status;
	} catch (e) {
		const err = e as Error & { status?: number };
		if (err.status === 404) {
			updateError.set(
				'In-panel updater is not available on this panel version. Run: sudo pgpanel update'
			);
		} else {
			updateError.set(err.message || 'Could not load update status');
		}
		updateStatus.set(null);
		return null;
	}
}

export async function checkForUpdates(): Promise<UpdateStatus | null> {
	updateChecking.set(true);
	updateError.set(null);
	try {
		const status = await api<UpdateStatus>('/api/updates/check', { method: 'POST' });
		updateStatus.set(status);
		return status;
	} catch (e) {
		const err = e as Error & { status?: number };
		if (err.status === 404) {
			updateError.set(
				'In-panel updater is not available on this panel version. Run: sudo pgpanel update'
			);
		} else {
			updateError.set(err.message || 'Update check failed');
		}
		return null;
	} finally {
		updateChecking.set(false);
	}
}

export interface ApplyUpdateResult {
	status: string;
	message?: string;
	version?: string;
	image?: string;
	poll_health?: boolean;
}

const RECOVERY_ENDPOINTS = ['/api/health', '/health', '/api/updates/status'] as const;

async function probePanelHealth(): Promise<boolean> {
	for (const path of RECOVERY_ENDPOINTS) {
		try {
			const res = await fetch(path, { credentials: 'include' });
			if (res.ok) {
				await res.json();
				return true;
			}
			if (res.status === 502 || res.status === 503) continue;
		} catch {
			/* network error — panel still down */
		}
	}
	return false;
}

function sleep(ms: number): Promise<void> {
	return new Promise((resolve) => setTimeout(resolve, ms));
}

/** Poll until the panel responds again after an in-place upgrade restart. */
export async function waitForPanelRecovery(opts?: {
	timeoutMs?: number;
	intervalMs?: number;
}): Promise<boolean> {
	const timeoutMs = opts?.timeoutMs ?? 5 * 60 * 1000;
	const intervalMs = opts?.intervalMs ?? 3000;
	const deadline = Date.now() + timeoutMs;

	while (Date.now() < deadline) {
		if (await probePanelHealth()) return true;
		await sleep(intervalMs);
	}
	return false;
}

export async function applyUpdate(): Promise<ApplyUpdateResult> {
	updateError.set(null);
	try {
		return await api<ApplyUpdateResult>('/api/updates/apply', { method: 'POST' });
	} catch (e) {
		const err = e as Error & { status?: number };
		// Panel may restart mid-request — treat as success if we got a gateway error.
		if (err.status === 502 || err.status === 503) {
			return {
				status: 'started',
				message: 'Update likely started — the panel is restarting. Refresh in a minute.'
			};
		}
		if (err.status === 404) {
			updateError.set(
				'In-panel updater is not available on this panel version. Run: sudo pgpanel update'
			);
		}
		throw e;
	}
}
