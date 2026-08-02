import type { BadgeVariant } from '$lib/components/ui/badge/badge.svelte';

export type ApiError = { error: string; code: string };

let csrfToken = '';

export function setCsrfToken(token: string) {
	csrfToken = token;
}

export function getCsrfToken() {
	return csrfToken;
}

export async function api<T>(path: string, options: RequestInit = {}): Promise<T> {
	const headers = new Headers(options.headers);
	if (!headers.has('Content-Type') && options.body) {
		headers.set('Content-Type', 'application/json');
	}
	if (csrfToken && options.method && options.method !== 'GET') {
		headers.set('x-csrf-token', csrfToken);
	}

	const res = await fetch(path, {
		...options,
		headers,
		credentials: 'include'
	});

	if (!res.ok) {
		let body: ApiError = { error: res.statusText, code: 'http_error' };
		try {
			body = await res.json();
		} catch {
			/* ignore */
		}
		throw Object.assign(new Error(body.error), { code: body.code, status: res.status });
	}

	if (res.status === 204) return undefined as T;
	return res.json() as Promise<T>;
}

export interface User {
	id: string;
	username: string;
	email: string;
}

export interface MeResponse {
	user: User;
	csrf_token: string;
}

export interface Cluster {
	id: string;
	name: string;
	slug: string;
	postgres_version: string;
	status: string;
	health: string;
	databasus_status: string;
	cpu_limit: number;
	memory_mb: number;
	storage_limit_gb: number;
	public_port: number | null;
	delete_protection: boolean;
	enable_backup: boolean;
	last_error: string | null;
	created_at: string;
	internal_hostname?: string;
	docker_container_name?: string;
}

export interface DashboardStats {
	cluster_count: number;
	healthy_count: number;
	degraded_count: number;
	database_count: number;
	active_operations: number;
	failed_operations: number;
}

export interface Operation {
	id: string;
	job_type: string;
	status: string;
	cluster_id: string | null;
	progress: number;
	error: string | null;
	result: unknown;
	created_at: string;
	updated_at: string;
}

export interface DatabaseRecord {
	id: string;
	name: string;
	owner_role: string;
	connection_limit: number | null;
}

export interface TableInfo {
	schema: string;
	name: string;
	table_type: string;
	row_estimate: number | null;
}

export interface AuditLog {
	id: number;
	actor_username: string | null;
	action: string;
	resource_type: string;
	resource_id: string | null;
	details: unknown;
	created_at: string;
}

/** Map cluster/job status → Badge variant */
export function statusVariant(status: string): BadgeVariant {
	const s = status.toLowerCase();
	if (s.includes('healthy') && !s.includes('warning')) return 'default';
	if (s.includes('warning') || s === 'degraded' || s === 'starting' || s === 'running' || s === 'queued')
		return 'secondary';
	if (s === 'failed' || s === 'deleting' || s === 'error' || s === 'unhealthy') return 'destructive';
	if (s === 'succeeded' || s === 'success') return 'default';
	return 'outline';
}

export function formatRelative(iso: string): string {
	try {
		const d = new Date(iso);
		return d.toLocaleString();
	} catch {
		return iso;
	}
}
