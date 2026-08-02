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
