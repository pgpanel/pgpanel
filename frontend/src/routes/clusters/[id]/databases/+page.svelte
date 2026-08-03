<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/stores';
	import {
		api,
		type ClusterConnectionRevealResponse,
		type DatabaseRecord,
		type RoleRecord
	} from '$lib/api';
	import PageHeader from '$lib/components/page-header.svelte';
	import EmptyState from '$lib/components/empty-state.svelte';
	import ConfirmDialog from '$lib/components/confirm-dialog.svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Input } from '$lib/components/ui/input/index.js';
	import { Label } from '$lib/components/ui/label/index.js';
	import { Switch } from '$lib/components/ui/switch/index.js';
	import { Checkbox } from '$lib/components/ui/checkbox/index.js';
	import { Badge } from '$lib/components/ui/badge/index.js';
	import * as Card from '$lib/components/ui/card/index.js';
	import * as Table from '$lib/components/ui/table/index.js';
	import * as Alert from '$lib/components/ui/alert/index.js';
	import * as Tabs from '$lib/components/ui/tabs/index.js';
	import { Separator } from '$lib/components/ui/separator/index.js';
	import { toast } from 'svelte-sonner';
	import { trackOperation } from '$lib/jobs';
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import {
		Alert02Icon,
		Copy01Icon,
		DatabaseIcon,
		Delete02Icon,
		InformationCircleIcon,
		UserIcon,
		SecurityCheckIcon,
		RefreshIcon
	} from '@hugeicons/core-free-icons';

	let databases = $state<DatabaseRecord[]>([]);
	let roles = $state<RoleRecord[]>([]);
	let error = $state('');
	let activeTab = $state('databases');

	// Database create
	let databaseName = $state('');
	let dbCreating = $state(false);

	// Role create
	let roleName = $state('');
	let roleGeneratePassword = $state(true);
	let rolePassword = $state('');
	let roleCreating = $state(false);
	let createdRoleName = $state('');
	let createdRolePassword = $state('');

	// Access editor
	let selectedRole = $state<RoleRecord | null>(null);
	let roleDatabases = $state<string[]>([]);
	let accessApiAvailable = $state(true);
	let accessLoading = $state(false);
	let accessSaving = $state(false);
	let revealingPassword = $state(false);
	let revealedAccess = $state<ClusterConnectionRevealResponse | null>(null);

	// Delete dialogs
	let deleteDbOpen = $state(false);
	let deleteDbName = $state<string | null>(null);
	let deleteRoleOpen = $state(false);
	let deleteRoleName = $state<string | null>(null);

	const id = $derived($page.params.id);
	const namePattern = '^[a-z][a-z0-9_]{2,62}$';
	async function loadDatabases() {
		databases = await api<DatabaseRecord[]>(`/api/clusters/${id}/databases`);
	}

	async function loadRoles() {
		roles = await api<RoleRecord[]>(`/api/clusters/${id}/roles`);
	}

	async function load() {
		await Promise.all([loadDatabases(), loadRoles()]);
	}

	onMount(() => {
		load().catch((e) => (error = e instanceof Error ? e.message : 'Failed to load'));
	});

	async function createDatabase(e: Event) {
		e.preventDefault();
		dbCreating = true;
		error = '';
		try {
			const res = await api<{ operation_id: string; message: string }>(
				`/api/clusters/${id}/databases`,
				{
					method: 'POST',
					body: JSON.stringify({ database_name: databaseName })
				}
			);
			const db = databaseName;
			databaseName = '';
			trackOperation(res.operation_id, {
				title: `Create database ${db}`,
				onDone: () => load(),
				onFail: () => load()
			});
			toast.success('Database creation queued');
		} catch (err) {
			error = err instanceof Error ? err.message : 'Failed';
			toast.error(error);
		} finally {
			dbCreating = false;
		}
	}

	function askDeleteDatabase(name: string) {
		deleteDbName = name;
		deleteDbOpen = true;
	}

	async function deleteDatabase() {
		if (!deleteDbName) return;
		const name = deleteDbName;
		try {
			const res = await api<{ operation_id: string }>(
				`/api/clusters/${id}/databases/${encodeURIComponent(name)}`,
				{ method: 'DELETE' }
			);
			trackOperation(res.operation_id, {
				title: `Delete database ${name}`,
				onDone: () => load(),
				onFail: () => load()
			});
			toast.success(`Delete queued for ${name}`);
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Delete failed');
		} finally {
			deleteDbName = null;
		}
	}

	async function copyText(text: string) {
		await navigator.clipboard.writeText(text);
		toast.success('Copied to clipboard');
	}

	async function createRole(e: Event) {
		e.preventDefault();
		roleCreating = true;
		error = '';
		try {
			const body: Record<string, unknown> = {
				role_name: roleName,
				generate_password: roleGeneratePassword
			};
			if (!roleGeneratePassword && rolePassword) {
				body.password = rolePassword;
			}
			const res = await api<{ id: string; role_name: string; password: string }>(
				`/api/clusters/${id}/roles`,
				{
					method: 'POST',
					body: JSON.stringify(body)
				}
			);
			createdRoleName = res.role_name;
			createdRolePassword = res.password;
			roleName = '';
			rolePassword = '';
			roleGeneratePassword = true;
			await load();
			toast.success(`Role ${res.role_name} created`);
			toast.message('Role password (copy now)', {
				description: res.role_name,
				duration: 15000
			});
		} catch (err) {
			error = err instanceof Error ? err.message : 'Failed';
			toast.error(error);
		} finally {
			roleCreating = false;
		}
	}

	function askDeleteRole(name: string) {
		deleteRoleName = name;
		deleteRoleOpen = true;
	}

	async function deleteRole() {
		if (!deleteRoleName) return;
		const name = deleteRoleName;
		try {
			await api(`/api/clusters/${id}/roles/${encodeURIComponent(name)}`, { method: 'DELETE' });
			toast.success(`Role ${name} deleted`);
			if (selectedRole?.name === name) {
				selectedRole = null;
				roleDatabases = [];
				revealedAccess = null;
			}
			await load();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Delete failed');
		} finally {
			deleteRoleName = null;
		}
	}

	async function selectRole(role: RoleRecord) {
		selectedRole = role;
		roleDatabases = [];
		revealedAccess = null;
		accessLoading = true;
		accessApiAvailable = true;
		try {
			const res = await api<{ databases: string[] }>(
				`/api/clusters/${id}/roles/${encodeURIComponent(role.name)}/databases`
			);
			roleDatabases = res.databases ?? [];
		} catch (err) {
			const status = (err as Error & { status?: number }).status;
			if (status === 404) {
				accessApiAvailable = false;
			} else {
				toast.error(err instanceof Error ? err.message : 'Could not load access');
			}
		} finally {
			accessLoading = false;
		}
	}

	async function revealSelectedRolePassword() {
		if (!selectedRole) return;
		const database = roleDatabases[0] ?? databases[0]?.name ?? 'postgres';
		revealingPassword = true;
		try {
			const result = await api<ClusterConnectionRevealResponse>(
				`/api/clusters/${id}/connection/reveal`,
				{
					method: 'POST',
					body: JSON.stringify({
						role: selectedRole.name,
						database,
						host_mode: 'internal'
					})
				}
			);
			revealedAccess = result;
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to reveal password');
		} finally {
			revealingPassword = false;
		}
	}

	function toggleDatabaseAccess(dbName: string, checked: boolean) {
		if (checked) {
			if (!roleDatabases.includes(dbName)) {
				roleDatabases = [...roleDatabases, dbName];
			}
		} else {
			roleDatabases = roleDatabases.filter((d) => d !== dbName);
		}
	}

	async function saveAccess() {
		if (!selectedRole || !accessApiAvailable) return;
		accessSaving = true;
		try {
			await api(`/api/clusters/${id}/roles/${encodeURIComponent(selectedRole.name)}/databases`, {
				method: 'PUT',
				body: JSON.stringify({ databases: roleDatabases })
			});
			toast.success('Database access updated');
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Save failed');
		} finally {
			accessSaving = false;
		}
	}
</script>

<PageHeader
	title="Databases & users"
	description="Manage application databases, login roles, and per-database connect access. Connection strings (with passwords) are on the cluster Overview page."
/>

{#if error}
	<Alert.Root variant="destructive" class="mb-4">
		<HugeiconsIcon icon={Alert02Icon} class="size-4" strokeWidth={2} />
		<Alert.Description>{error}</Alert.Description>
	</Alert.Root>
{/if}

<Tabs.Root bind:value={activeTab} class="space-y-6">
	<Tabs.List>
		<Tabs.Trigger value="databases">
			<HugeiconsIcon icon={DatabaseIcon} class="size-4" strokeWidth={2} />
			Databases
		</Tabs.Trigger>
		<Tabs.Trigger value="users">
			<HugeiconsIcon icon={UserIcon} class="size-4" strokeWidth={2} />
			Users (roles)
		</Tabs.Trigger>
	</Tabs.List>

	<Tabs.Content value="databases" class="space-y-6">
		<div class="grid gap-6 lg:grid-cols-[minmax(280px,360px)_1fr]">
			<Card.Root class="h-fit border-border/60">
				<Card.Header>
					<Card.Title>Create database</Card.Title>
					<Card.Description>
						Owner is the cluster user. Assign extra users on the Users tab. Names must match
						<code class="font-mono text-xs">^[a-z][a-z0-9_]&#123;2,62&#125;$</code>
					</Card.Description>
				</Card.Header>
				<form onsubmit={createDatabase}>
					<Card.Content class="space-y-4">
						<div class="space-y-2">
							<Label for="db-name">Database name</Label>
							<Input
								id="db-name"
								bind:value={databaseName}
								pattern={namePattern}
								required
								class="font-mono"
								placeholder="my_app"
							/>
						</div>
					</Card.Content>
					<Card.Footer>
						<Button type="submit" class="w-full" disabled={dbCreating}>
							{dbCreating ? 'Creating…' : 'Create database'}
						</Button>
					</Card.Footer>
				</form>
			</Card.Root>

			<Card.Root class="border-border/60">
				<Card.Header class="flex-row items-center justify-between gap-4">
					<div>
						<Card.Title>Databases</Card.Title>
						<Card.Description>{databases.length} tracked in this cluster</Card.Description>
					</div>
					<Button variant="outline" size="sm" onclick={() => loadDatabases().catch(() => {})}>
						<HugeiconsIcon icon={RefreshIcon} class="size-4" strokeWidth={2} />
						Refresh
					</Button>
				</Card.Header>
				{#if databases.length === 0}
					<Card.Content>
						<EmptyState
							title="No application databases"
							description="Create one here, then get connection strings on the cluster Overview page."
						/>
					</Card.Content>
				{:else}
					<Card.Content class="p-0">
						<Table.Root>
							<Table.Header>
								<Table.Row>
									<Table.Head>Database</Table.Head>
									<Table.Head>Owner role</Table.Head>
									<Table.Head class="text-right">Actions</Table.Head>
								</Table.Row>
							</Table.Header>
							<Table.Body>
								{#each databases as d (d.id)}
									<Table.Row>
										<Table.Cell class="font-mono">{d.name}</Table.Cell>
										<Table.Cell class="font-mono text-muted-foreground">{d.owner_role}</Table.Cell>
										<Table.Cell class="text-right">
											<Button
												variant="ghost"
												size="sm"
												onclick={() => askDeleteDatabase(d.name)}
											>
												<HugeiconsIcon icon={Delete02Icon} class="size-4" strokeWidth={2} />
												Delete
											</Button>
										</Table.Cell>
									</Table.Row>
								{/each}
							</Table.Body>
						</Table.Root>
					</Card.Content>
				{/if}
			</Card.Root>
		</div>
	</Tabs.Content>

	<Tabs.Content value="users" class="space-y-6">
		{#if createdRolePassword}
			<Alert.Root class="border-amber-500/40 bg-amber-500/10 text-amber-50">
				<HugeiconsIcon icon={InformationCircleIcon} class="size-4" strokeWidth={2} />
				<Alert.Title>Credentials (copy now)</Alert.Title>
				<Alert.Description class="mt-2 space-y-3">
					<p class="text-xs opacity-80">
						Password for role <span class="font-mono font-medium">{createdRoleName}</span> is shown
						once here. You can also reveal it later from the cluster Overview connection string.
					</p>
					<div>
						<p class="mb-1 text-xs font-medium">Role password</p>
						<div class="flex flex-wrap items-center gap-2">
							<code class="rounded-md bg-black/40 px-3 py-2 font-mono text-sm break-all"
								>{createdRolePassword}</code
							>
							<Button
								size="sm"
								variant="secondary"
								onclick={() => copyText(createdRolePassword)}
							>
								<HugeiconsIcon icon={Copy01Icon} class="size-4" strokeWidth={2} />
								Copy
							</Button>
						</div>
					</div>
				</Alert.Description>
			</Alert.Root>
		{/if}

		<div class="grid gap-6 lg:grid-cols-[minmax(280px,360px)_1fr]">
			<Card.Root class="h-fit border-border/60">
				<Card.Header>
					<Card.Title>Create role</Card.Title>
					<Card.Description>Non-superuser login role for application access</Card.Description>
				</Card.Header>
				<form onsubmit={createRole}>
					<Card.Content class="space-y-4">
						<div class="space-y-2">
							<Label for="role-name">Role name</Label>
							<Input
								id="role-name"
								bind:value={roleName}
								pattern={namePattern}
								required
								class="font-mono"
								placeholder="app_user"
							/>
						</div>
						<label
							class="flex items-center justify-between rounded-lg border border-border/60 p-3 text-sm"
						>
							<span>Generate password</span>
							<Switch bind:checked={roleGeneratePassword} />
						</label>
						{#if !roleGeneratePassword}
							<div class="space-y-2">
								<Label for="role-password">Password</Label>
								<Input
									id="role-password"
									type="password"
									bind:value={rolePassword}
									autocomplete="new-password"
									required
								/>
							</div>
						{/if}
					</Card.Content>
					<Card.Footer>
						<Button type="submit" class="w-full" disabled={roleCreating}>
							{roleCreating ? 'Creating…' : 'Create role'}
						</Button>
					</Card.Footer>
				</form>
			</Card.Root>

			<Card.Root class="border-border/60">
				<Card.Header class="flex-row items-center justify-between gap-4">
					<div>
						<Card.Title>Roles</Card.Title>
						<Card.Description>{roles.length} roles in this cluster</Card.Description>
					</div>
					<Button variant="outline" size="sm" onclick={() => loadRoles().catch(() => {})}>
						<HugeiconsIcon icon={RefreshIcon} class="size-4" strokeWidth={2} />
						Refresh
					</Button>
				</Card.Header>
				{#if roles.length === 0}
					<Card.Content>
						<EmptyState
							title="No roles yet"
							description="Create a role to grant application login access."
						/>
					</Card.Content>
				{:else}
					<Card.Content class="p-0">
						<Table.Root>
							<Table.Header>
								<Table.Row>
									<Table.Head>Role</Table.Head>
									<Table.Head>Flags</Table.Head>
									<Table.Head class="text-right">Actions</Table.Head>
								</Table.Row>
							</Table.Header>
							<Table.Body>
								{#each roles as r (r.id)}
									<Table.Row
										class={selectedRole?.id === r.id ? 'bg-muted/40' : 'cursor-pointer'}
										onclick={() => selectRole(r)}
									>
										<Table.Cell class="font-mono font-medium">{r.name}</Table.Cell>
										<Table.Cell>
											<div class="flex flex-wrap gap-1">
												{#if r.is_superuser}
													<Badge variant="secondary">superuser</Badge>
												{/if}
												{#if r.can_login}
													<Badge variant="default">login</Badge>
												{:else}
													<Badge variant="outline">no login</Badge>
												{/if}
											</div>
										</Table.Cell>
										<Table.Cell class="text-right">
											<Button
												variant="ghost"
												size="sm"
												onclick={(e) => {
													e.stopPropagation();
													askDeleteRole(r.name);
												}}
											>
												<HugeiconsIcon icon={Delete02Icon} class="size-4" strokeWidth={2} />
												Delete
											</Button>
										</Table.Cell>
									</Table.Row>
								{/each}
							</Table.Body>
						</Table.Root>
					</Card.Content>
				{/if}
			</Card.Root>
		</div>

		<Card.Root class="border-border/60">
			<Card.Header>
				<Card.Title class="flex items-center gap-2">
					<HugeiconsIcon icon={SecurityCheckIcon} class="size-5 text-primary" strokeWidth={2} />
					Database access
				</Card.Title>
				<Card.Description>
					{#if selectedRole}
						Connect privileges for <span class="font-mono">{selectedRole.name}</span>
					{:else}
						Select a role above to edit which databases it can connect to
					{/if}
				</Card.Description>
			</Card.Header>
			<Card.Content>
				{#if !selectedRole}
					<p class="text-sm text-muted-foreground">
						Click a role in the table to configure database access.
					</p>
				{:else if accessLoading}
					<p class="text-sm text-muted-foreground">Loading access…</p>
				{:else if !accessApiAvailable}
					<Alert.Root>
						<HugeiconsIcon icon={Alert02Icon} class="size-4" strokeWidth={2} />
						<Alert.Description>
							Access editing requires panel update. Role and database management still works.
						</Alert.Description>
					</Alert.Root>
					<div class="mt-4 space-y-2 opacity-50">
						{#each databases as d (d.id)}
							<label class="flex items-center gap-3 rounded-lg border border-border/60 p-3 text-sm">
								<Checkbox disabled checked={false} />
								<span class="font-mono">{d.name}</span>
							</label>
						{/each}
					</div>
				{:else}
					{#if databases.length === 0}
						<p class="text-sm text-muted-foreground">No databases to assign.</p>
					{:else}
						<div class="space-y-2">
							{#each databases as d (d.id)}
								<label
									class="flex items-center gap-3 rounded-lg border border-border/60 p-3 text-sm transition-colors hover:bg-muted/30"
								>
									<Checkbox
										checked={roleDatabases.includes(d.name)}
										onCheckedChange={(v) => toggleDatabaseAccess(d.name, v === true)}
									/>
									<span class="font-mono">{d.name}</span>
									{#if d.owner_role === selectedRole.name}
										<Badge variant="outline" class="ml-auto">owner</Badge>
									{/if}
								</label>
							{/each}
						</div>
						<Separator class="my-4" />
					{/if}
					<div class="flex flex-wrap items-center gap-2" class:mt-4={databases.length === 0}>
						{#if databases.length > 0}
							<Button onclick={saveAccess} disabled={accessSaving}>
								{accessSaving ? 'Saving…' : 'Save access'}
							</Button>
						{/if}
						<Button
							variant="outline"
							onclick={revealSelectedRolePassword}
							disabled={revealingPassword}
						>
							{revealingPassword ? 'Revealing…' : 'Reveal password'}
						</Button>
					</div>
					{#if revealedAccess}
						<Alert.Root class="mt-4 border-amber-500/40 bg-amber-500/10">
							<HugeiconsIcon icon={InformationCircleIcon} class="size-4" strokeWidth={2} />
							<Alert.Title>Password for {revealedAccess.role}</Alert.Title>
							<Alert.Description class="mt-2 space-y-3">
								{#if revealedAccess.warning}
									<p class="text-sm">{revealedAccess.warning}</p>
								{/if}
								<div class="space-y-1.5">
									<p class="text-xs font-medium">Password</p>
									<div class="flex flex-wrap items-center gap-2">
										<code class="rounded-md bg-black/40 px-3 py-2 font-mono text-sm break-all"
											>{revealedAccess.password}</code
										>
										<Button
											variant="secondary"
											size="sm"
											onclick={() => copyText(revealedAccess!.password)}
										>
											<HugeiconsIcon icon={Copy01Icon} class="size-4" strokeWidth={2} />
											Copy
										</Button>
									</div>
								</div>
								<div class="space-y-1.5">
									<p class="text-xs font-medium">Connection string</p>
									<div class="flex flex-wrap items-center gap-2">
										<code class="rounded-md bg-black/40 px-3 py-2 font-mono text-xs break-all"
											>{revealedAccess.connection_string}</code
										>
										<Button
											variant="secondary"
											size="sm"
											onclick={() => copyText(revealedAccess!.connection_string)}
										>
											<HugeiconsIcon icon={Copy01Icon} class="size-4" strokeWidth={2} />
											Copy
										</Button>
									</div>
								</div>
							</Alert.Description>
						</Alert.Root>
					{/if}
				{/if}
			</Card.Content>
		</Card.Root>
	</Tabs.Content>
</Tabs.Root>

<ConfirmDialog
	bind:open={deleteDbOpen}
	title="Delete database?"
	description={deleteDbName ? `This will queue deletion of "${deleteDbName}".` : ''}
	confirmLabel="Delete"
	variant="destructive"
	onConfirm={deleteDatabase}
/>

<ConfirmDialog
	bind:open={deleteRoleOpen}
	title="Delete role?"
	description={deleteRoleName ? `Drop role "${deleteRoleName}" from PostgreSQL.` : ''}
	confirmLabel="Delete"
	variant="destructive"
	onConfirm={deleteRole}
/>
