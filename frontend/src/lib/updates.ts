import { writable } from 'svelte/store';
import { api, type UpdateStatus } from './api';

export const updateStatus = writable<UpdateStatus | null>(null);
export const updateChecking = writable(false);

export async function fetchUpdateStatus(): Promise<UpdateStatus | null> {
	try {
		const status = await api<UpdateStatus>('/api/updates/status');
		updateStatus.set(status);
		return status;
	} catch {
		updateStatus.set(null);
		return null;
	}
}

export async function checkForUpdates(): Promise<UpdateStatus | null> {
	updateChecking.set(true);
	try {
		const status = await api<UpdateStatus>('/api/updates/check', { method: 'POST' });
		updateStatus.set(status);
		return status;
	} catch {
		return null;
	} finally {
		updateChecking.set(false);
	}
}

export async function applyUpdate(): Promise<void> {
	await api('/api/updates/apply', { method: 'POST' });
}
