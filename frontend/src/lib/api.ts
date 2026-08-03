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

export type UserRole = 'owner' | 'admin' | 'operator' | 'viewer';

export interface User {
	id: string;
	username: string;
	email: string;
	role: UserRole;
	display_name: string;
	enabled: boolean;
	created_at?: string;
	last_login_at?: string | null;
}

export interface CreateUserRequest {
	username: string;
	email: string;
	password: string;
	role: UserRole;
	display_name?: string;
}

export interface UpdateUserRequest {
	email?: string;
	role?: UserRole;
	display_name?: string;
	enabled?: boolean;
	password?: string;
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
	databasus_status?: string;
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
	node_id?: string | null;
}

export type NodeKind = 'local' | 'remote';
export type NodeStatus = 'online' | 'offline' | 'unknown';

export interface Node {
	id: string;
	name: string;
	slug: string;
	kind: NodeKind;
	docker_host_display: string | null;
	status: NodeStatus;
	last_seen_at: string | null;
	last_error: string | null;
	max_clusters: number | null;
	cluster_count: number;
	notes: string | null;
	is_default: boolean;
	labels: Record<string, unknown>;
	created_at: string;
	updated_at: string;
}

export interface WafPolicyConfig {
	rate_limit_per_minute: number;
	login_rate_limit_per_minute: number;
	api_rate_limit_per_minute: number;
	max_body_bytes: number;
	block_empty_user_agent: boolean;
	blocked_user_agents: string[];
	allowed_ips: string[];
	denied_ips: string[];
	blocked_paths: string[];
	challenge_suspicious: boolean;
	geo_block_countries: string[];
	enable_security_headers: boolean;
	hsts_max_age: number;
	csp_mode: string;
	fail_closed_on_deny: boolean;
}

export interface WafPolicy {
	id: string;
	name: string;
	enabled: boolean;
	is_active: boolean;
	version: number;
	config: WafPolicyConfig;
	notes: string | null;
	created_at: string;
	updated_at: string;
}

export interface WafChangeEntry {
	id: number;
	policy_id: string;
	version_from: number | null;
	version_to: number;
	actor_username: string | null;
	change_summary: string;
	before_json: unknown;
	after_json: unknown;
	reason: string | null;
	ip_address: string | null;
	created_at: string;
}

export interface BackupSchedule {
	id: string;
	cluster_id: string;
	cron: string;
	kind: string;
	database_name: string;
	enabled: boolean;
	retention_days: number;
	keep_count: number;
	compression_level: number;
	dump_format: string;
	schema_only: boolean;
	exclude_schemas: string;
	exclude_tables: string;
	include_schemas: string;
	jobs: number;
	notify_on_success: boolean;
	notify_on_failure: boolean;
	verify_after: boolean;
	window_start_hour: number | null;
	window_end_hour: number | null;
	pause_until: string | null;
	next_run_at: string | null;
	description: string | null;
	last_run_at: string | null;
	created_at: string;
	updated_at: string;
}

export interface GlobalBackupPolicy {
	retention_days: number;
	keep_count: number;
	schedule_hour: number;
	cron_default: string;
	compression_level: number;
	dump_format: string;
	verify_after: boolean;
	keep_local_copy: boolean;
	notify_webhook: string;
	notify_on_success: boolean;
	notify_on_failure: boolean;
	exclude_schemas_default: string;
	wal_archiving_default: boolean;
	parallel_jobs: number;
	encrypt: boolean;
}

export interface DashboardStats {
	cluster_count: number;
	healthy_count: number;
	degraded_count: number;
	database_count: number;
	active_operations: number;
	failed_operations: number;
	node_count: number;
	open_alerts: number;
	replica_count: number;
	backup_destinations: number;
}

export interface BackupDestination {
	id: string;
	name: string;
	slug: string;
	storage_type: string;
	endpoint: string;
	region: string;
	bucket: string;
	prefix: string;
	path_style: boolean;
	tls_verify: boolean;
	encrypt_backups: boolean;
	compression_level: number;
	enabled: boolean;
	is_default: boolean;
	access_key_set: boolean;
	secret_key_set: boolean;
	notes: string | null;
	last_test_at: string | null;
	last_test_ok: boolean | null;
	last_test_error: string | null;
	created_at: string;
	updated_at: string;
}

export interface UpsertBackupDestinationRequest {
	name: string;
	storage_type: string;
	endpoint?: string;
	region?: string;
	bucket?: string;
	prefix?: string;
	path_style?: boolean;
	tls_verify?: boolean;
	encrypt_backups?: boolean;
	compression_level?: number;
	access_key?: string;
	secret_key?: string;
	notes?: string | null;
	set_default?: boolean;
	enabled?: boolean;
	allowed_node_ids?: string[];
}

export interface ClusterBackupTarget {
	id: string;
	cluster_id: string;
	destination_id: string;
	destination_name: string;
	enabled: boolean;
	priority: number;
	include_databases: string;
	exclude_databases: string;
	cron: string;
	retention_days: number;
	keep_count: number;
	schema_only: boolean;
	verify_after: boolean;
	compression_level: number | null;
	dump_format: string;
	exclude_schemas: string;
	exclude_tables: string;
	parallel_jobs: number;
	notify_on_success: boolean;
	notify_on_failure: boolean;
	window_start_hour: number | null;
	window_end_hour: number | null;
	last_run_at: string | null;
	last_status: string | null;
}

export interface UpsertClusterBackupTargetRequest {
	destination_id: string;
	enabled?: boolean;
	priority?: number;
	include_databases?: string;
	exclude_databases?: string;
	cron?: string;
	retention_days?: number;
	keep_count?: number;
	schema_only?: boolean;
	verify_after?: boolean;
	compression_level?: number | null;
	dump_format?: string;
	exclude_schemas?: string;
	exclude_tables?: string;
	parallel_jobs?: number;
	notify_on_success?: boolean;
	notify_on_failure?: boolean;
	window_start_hour?: number | null;
	window_end_hour?: number | null;
}

export interface ClusterReplica {
	id: string;
	primary_cluster_id: string;
	replica_cluster_id: string | null;
	name: string;
	mode: string;
	target_node_id: string;
	sync_cron: string;
	status: string;
	lag_seconds: number | null;
	last_sync_at: string | null;
	last_error: string | null;
	auto_failover: boolean;
	promote_protection: boolean;
	enabled: boolean;
	created_at: string;
	updated_at: string;
}

export interface CreateReplicaRequest {
	name: string;
	target_node_id: string;
	mode?: string;
	sync_cron?: string;
	auto_failover?: boolean;
	provision_now?: boolean;
}

export interface AlertRule {
	id: string;
	name: string;
	enabled: boolean;
	severity: string;
	metric: string;
	operator: string;
	threshold: number;
	duration_seconds: number;
	scope: string;
	scope_id: string | null;
	notify_channels: string;
	cooldown_seconds: number;
}

export interface Alert {
	id: string;
	rule_id: string | null;
	severity: string;
	title: string;
	message: string;
	resource_type: string | null;
	resource_id: string | null;
	status: string;
	fired_at: string;
	acked_at: string | null;
	resolved_at: string | null;
}

export interface MonitoringNodeRow {
	node_id: string;
	name: string;
	status: string;
	cluster_count: number;
	avg_cpu?: number;
	cpu_percent?: number;
	memory_mb?: number;
	max_clusters?: number | null;
}

export interface MonitoringOverview {
	cluster_count: number;
	healthy_count: number;
	open_alerts: number;
	avg_cpu: number;
	avg_memory_mb: number;
	backups_last_24h: number;
	failed_backups_24h: number;
	replica_healthy: number;
	replica_total: number;
	series: MonitoringPoint[];
	top_clusters: ClusterLoadRow[];
	max_cpu_24h?: number;
	max_memory_mb_24h?: number;
	node_count?: number;
	online_nodes?: number;
	operations_failed_24h?: number;
	operations_running?: number;
	databases_total?: number;
	/** 0..1 fraction from backend */
	backup_success_rate?: number;
	nodes?: MonitoringNodeRow[];
	hours?: number;
}

export interface FleetRemoteAccess {
	enabled: boolean;
	public_base_url: string | null;
	public_name?: string;
	has_token?: boolean;
	token_prefix?: string | null;
	join_url: string | null;
	join_token: string | null;
	updated_at?: string;
}

export interface FleetInvite {
	id: string;
	token: string;
	token_prefix?: string;
	join_url: string;
	label?: string;
	created_at: string;
	expires_at: string | null;
	used_at: string | null;
	revoked_at?: string | null;
	used?: boolean;
	revoked?: boolean;
}

export interface FleetPeer {
	id: string;
	name: string;
	base_url: string;
	status: string;
	last_seen_at: string | null;
	cluster_count?: number;
	advertised_capacity?: number | null;
	max_clusters?: number | null;
	last_ping_ms?: number | null;
}

export interface FleetJoinRequest {
	link?: string;
	base_url?: string;
	token?: string;
	local_name?: string;
}

export interface UpdateStatus {
	current_version: string;
	image_version?: string;
	running_image_tag?: string | null;
	latest_version: string | null;
	latest_url?: string | null;
	update_available: boolean;
	channel?: string;
	last_checked_at: string | null;
	can_apply?: boolean;
	changelog?: string | null;
}

export interface MonitoringPoint {
	at: string;
	cpu: number;
	memory_mb: number;
}

export interface ClusterLoadRow {
	cluster_id: string;
	name: string;
	cpu_percent: number;
	memory_usage_mb: number;
	status: string;
}

export interface ApiTokenInfo {
	id: string;
	name: string;
	token_prefix: string;
	role: string;
	scopes: string;
	expires_at: string | null;
	last_used_at: string | null;
	revoked_at: string | null;
	created_at: string;
}

export interface ApiTokenCreated {
	id: string;
	name: string;
	token: string;
	token_prefix: string;
	role: string;
	scopes: string;
	expires_at: string | null;
}

export interface CreateApiTokenRequest {
	name: string;
	role?: string;
	scopes?: string;
	expires_days?: number | null;
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

export interface WalPgStatus {
	archive_mode: string | null;
	wal_level: string | null;
	last_archived_wal: string | null;
	failed_count: number;
	message: string | null;
}

export interface WalStreamStatus {
	enabled: boolean;
	status: string;
	archive_dir: string | null;
	retention_days: number;
	last_segment: string | null;
	last_synced_at: string | null;
	segment_count: number;
	total_bytes: number;
	timeline: string | null;
	pg: WalPgStatus;
	searchable: boolean;
}

/** Normalize GET /wal responses (flat preferred; nested `stream` accepted for older builds). */
export function normalizeWalStatus(raw: unknown): WalStreamStatus {
	const obj = (raw && typeof raw === 'object' ? raw : {}) as Record<string, unknown>;
	const stream =
		obj.stream && typeof obj.stream === 'object'
			? (obj.stream as Record<string, unknown>)
			: null;
	const src = stream ?? obj;
	const pgRaw =
		(obj.pg && typeof obj.pg === 'object' ? obj.pg : null) ??
		(src.pg && typeof src.pg === 'object' ? src.pg : null);
	const pgSrc = (pgRaw ?? {}) as Record<string, unknown>;
	const segmentCount = Number(src.segment_count ?? 0) || 0;
	const enabled = Boolean(obj.enabled ?? src.enabled ?? false);
	return {
		enabled,
		status: String(src.status ?? (enabled ? 'unknown' : 'disabled')),
		archive_dir: (src.archive_dir as string | null | undefined) ?? null,
		retention_days: Number(src.retention_days ?? 14) || 14,
		last_segment: (src.last_segment as string | null | undefined) ?? null,
		last_synced_at: (src.last_synced_at as string | null | undefined) ?? null,
		segment_count: segmentCount,
		total_bytes: Number(src.total_bytes ?? 0) || 0,
		timeline:
			src.timeline == null || src.timeline === ''
				? null
				: String(src.timeline),
		searchable: Boolean(obj.searchable ?? src.searchable ?? segmentCount > 0),
		pg: {
			archive_mode: (pgSrc.archive_mode as string | null | undefined) ?? null,
			wal_level: (pgSrc.wal_level as string | null | undefined) ?? null,
			last_archived_wal: (pgSrc.last_archived_wal as string | null | undefined) ?? null,
			failed_count: Number(pgSrc.failed_count ?? 0) || 0,
			message: (pgSrc.message as string | null | undefined) ?? null
		}
	};
}

export interface WalSegment {
	id: string;
	filename: string;
	timeline: string;
	size_bytes: number;
	archived_at: string | null;
	synced_at: string | null;
	checksum_sha256: string | null;
}

export interface WalSegmentsResponse {
	segments: WalSegment[];
	total?: number;
}

export interface WalEnableRequest {
	retention_days?: number;
	compress?: boolean;
}

export interface WalPitrRequest {
	target_time: string;
	confirm_cluster_name: string;
}

export interface OperationIdResponse {
	operation_id: string;
}

export interface ClusterConnectionInfo {
	cluster_id: string;
	internal_host: string;
	public_port: number | null;
	default_port: number;
	default_role: string;
	default_database: string;
	roles: string[];
	databases: { name: string; owner_role: string }[];
}

export interface ClusterConnectionRevealResponse {
	role: string;
	database: string;
	host: string;
	port: number;
	password: string;
	connection_string: string;
	warning: string;
}

export interface DatabaseRecord {
	id: string;
	name: string;
	owner_role: string;
	connection_limit: number | null;
}

export interface RoleRecord {
	id: string;
	cluster_id: string;
	name: string;
	is_superuser: boolean;
	can_login: boolean;
	connection_limit: number | null;
	created_at: string;
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
export function statusVariant(status: string | null | undefined): BadgeVariant {
	if (!status) return 'outline';
	const s = status.toLowerCase();
	if (s.includes('healthy') && !s.includes('warning')) return 'default';
	if (s.includes('warning') || s === 'degraded' || s === 'starting' || s === 'running' || s === 'queued')
		return 'secondary';
	if (s === 'failed' || s === 'deleting' || s === 'error' || s === 'unhealthy') return 'destructive';
	if (s === 'succeeded' || s === 'success') return 'default';
	return 'outline';
}

/** Display cluster capacity — empty/null/<=0 max means unlimited */
export function formatNodeCapacity(count: number, max: number | null | undefined): string {
	const unlimited = max == null || max <= 0;
	return unlimited ? `${count} / Unlimited` : `${count} / ${max}`;
}

export function isUnlimitedCapacity(max: number | null | undefined): boolean {
	return max == null || max <= 0;
}

export function formatRelative(iso: string): string {
	try {
		const d = new Date(iso);
		return d.toLocaleString();
	} catch {
		return iso;
	}
}
