<script lang="ts">
	import { onMount } from 'svelte';
	import { api, type User, type UserRole, formatRelative } from '$lib/api';
	import { user as currentUser } from '$lib/auth';
	import PageHeader from '$lib/components/page-header.svelte';
	import StatusBadge from '$lib/components/status-badge.svelte';
	import EmptyState from '$lib/components/empty-state.svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Input } from '$lib/components/ui/input/index.js';
	import { Label } from '$lib/components/ui/label/index.js';
	import * as Card from '$lib/components/ui/card/index.js';
	import * as Table from '$lib/components/ui/table/index.js';
	import * as Alert from '$lib/components/ui/alert/index.js';
	import * as Select from '$lib/components/ui/select/index.js';
	import { Badge } from '$lib/components/ui/badge/index.js';
	import { toast } from 'svelte-sonner';
	import { HugeiconsIcon } from '@hugeicons/svelte';
	import {
		Alert02Icon,
		UserGroupIcon,
		PlusSignIcon,
		RefreshIcon,
		Delete02Icon,
		InformationCircleIcon
	} from '@hugeicons/core-free-icons';

	const roles: UserRole[] = ['owner', 'admin', 'operator', 'viewer'];

	let users = $state<User[]>([]);
	let error = $state('');
	let loading = $state(true);
	let creating = $state(false);

	let username = $state('');
	let email = $state('');
	let password = $state('');
	let display_name = $state('');
	let role = $state<UserRole>('operator');

	const isAdmin = $derived(
		$currentUser?.role === 'admin' || $currentUser?.role === 'owner'
	);

	async function load() {
		users = await api<User[]>('/api/users');
	}

	onMount(() => {
		load()
			.catch((e) => (error = e instanceof Error ? e.message : 'Failed to load users'))
			.finally(() => (loading = false));
	});

	async function createUser(e: Event) {
		e.preventDefault();
		creating = true;
		error = '';
		try {
			await api<User>('/api/users', {
				method: 'POST',
				body: JSON.stringify({
					username,
					email,
					password,
					role,
					display_name: display_name || undefined
				})
			});
			toast.success('User created');
			username = '';
			email = '';
			password = '';
			display_name = '';
			role = 'operator';
			await load();
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to create user';
			toast.error(error);
		} finally {
			creating = false;
		}
	}

	async function updateRole(u: User, newRole: UserRole) {
		try {
			await api<User>(`/api/users/${u.id}`, {
				method: 'PUT',
				body: JSON.stringify({ role: newRole })
			});
			toast.success(`Role updated for ${u.username}`);
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Update failed');
		}
	}

	async function toggleEnabled(u: User) {
		try {
			await api<User>(`/api/users/${u.id}`, {
				method: 'PUT',
				body: JSON.stringify({ enabled: !u.enabled })
			});
			toast.success(u.enabled ? 'User disabled' : 'User enabled');
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Update failed');
		}
	}

	async function deleteUser(u: User) {
		if (!confirm(`Delete user "${u.username}"? This cannot be undone.`)) return;
		try {
			await api(`/api/users/${u.id}`, { method: 'DELETE' });
			toast.success('User deleted');
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Delete failed');
		}
	}
</script>

<PageHeader title="Users" description="RBAC user administration — admin and owner access only">
	{#snippet actions()}
		<Button variant="outline" size="sm" onclick={() => load()}>
			<HugeiconsIcon icon={RefreshIcon} class="size-4" strokeWidth={2} />
			Refresh
		</Button>
	{/snippet}
</PageHeader>

<Alert.Root class="mb-4 border-border/60">
	<HugeiconsIcon icon={InformationCircleIcon} class="size-4" strokeWidth={2} />
	<Alert.Description>
		User management requires <strong>admin</strong> or <strong>owner</strong> role. Operators and viewers
		cannot access this page.
	</Alert.Description>
</Alert.Root>

{#if !isAdmin && !loading}
	<Alert.Root variant="destructive" class="mb-4">
		<HugeiconsIcon icon={Alert02Icon} class="size-4" strokeWidth={2} />
		<Alert.Description>Your account does not have permission to manage users.</Alert.Description>
	</Alert.Root>
{/if}

{#if error}
	<Alert.Root variant="destructive" class="mb-4">
		<HugeiconsIcon icon={Alert02Icon} class="size-4" strokeWidth={2} />
		<Alert.Description>{error}</Alert.Description>
	</Alert.Root>
{/if}

<div class="mb-6 grid gap-6 lg:grid-cols-2">
	<Card.Root class="border-border/60">
		<Card.Header>
			<Card.Title class="flex items-center gap-2">
				<HugeiconsIcon icon={UserGroupIcon} class="size-5 text-primary" strokeWidth={2} />
				Create user
			</Card.Title>
			<Card.Description>Add a panel account with role-based permissions</Card.Description>
		</Card.Header>
		<Card.Content>
			<form class="space-y-4" onsubmit={createUser}>
				<div class="space-y-2">
					<Label for="username">Username</Label>
					<Input id="username" bind:value={username} required minlength={3} maxlength={64} />
				</div>
				<div class="space-y-2">
					<Label for="email">Email</Label>
					<Input id="email" type="email" bind:value={email} required />
				</div>
				<div class="space-y-2">
					<Label for="password">Password</Label>
					<Input id="password" type="password" bind:value={password} required autocomplete="new-password" />
				</div>
				<div class="space-y-2">
					<Label for="display-name">Display name <span class="text-muted-foreground">(optional)</span></Label>
					<Input id="display-name" bind:value={display_name} />
				</div>
				<div class="space-y-2">
					<Label>Role</Label>
					<Select.Root type="single" bind:value={role}>
						<Select.Trigger class="w-full capitalize">{role}</Select.Trigger>
						<Select.Content>
							{#each roles as r (r)}
								<Select.Item value={r} label={r} class="capitalize">{r}</Select.Item>
							{/each}
						</Select.Content>
					</Select.Root>
				</div>
				<Button type="submit" disabled={creating || !isAdmin} class="w-full">
					<HugeiconsIcon icon={PlusSignIcon} class="size-4" strokeWidth={2} />
					{creating ? 'Creating…' : 'Create user'}
				</Button>
			</form>
		</Card.Content>
	</Card.Root>

	<Card.Root class="border-border/60">
		<Card.Header>
			<Card.Title>Accounts</Card.Title>
			<Card.Description>{users.length} registered users</Card.Description>
		</Card.Header>
		<Card.Content class="p-0">
			{#if !loading && users.length === 0}
				<div class="p-6">
					<EmptyState title="No users" description="Create the first additional account above." />
				</div>
			{:else}
				<Table.Root>
					<Table.Header>
						<Table.Row class="hover:bg-transparent">
							<Table.Head>User</Table.Head>
							<Table.Head>Role</Table.Head>
							<Table.Head>Status</Table.Head>
							<Table.Head class="text-right">Actions</Table.Head>
						</Table.Row>
					</Table.Header>
					<Table.Body>
						{#each users as u (u.id)}
							<Table.Row>
								<Table.Cell>
									<div class="font-medium">{u.display_name || u.username}</div>
									<div class="text-xs text-muted-foreground">@{u.username} · {u.email}</div>
									{#if u.last_login_at}
										<div class="text-xs text-muted-foreground">
											Last login {formatRelative(u.last_login_at)}
										</div>
									{/if}
								</Table.Cell>
								<Table.Cell>
									<Select.Root
										type="single"
										value={u.role}
										onValueChange={(v) => v && updateRole(u, v as UserRole)}
									>
										<Select.Trigger class="h-8 w-28 capitalize text-xs">{u.role}</Select.Trigger>
										<Select.Content>
											{#each roles as r (r)}
												<Select.Item value={r} label={r} class="capitalize">{r}</Select.Item>
											{/each}
										</Select.Content>
									</Select.Root>
								</Table.Cell>
								<Table.Cell>
									{#if u.enabled}
										<Badge variant="default">enabled</Badge>
									{:else}
										<StatusBadge status="disabled" />
									{/if}
								</Table.Cell>
								<Table.Cell class="text-right">
									<div class="flex justify-end gap-1">
										<Button
											variant="ghost"
											size="sm"
											disabled={u.id === $currentUser?.id}
											onclick={() => toggleEnabled(u)}
										>
											{u.enabled ? 'Disable' : 'Enable'}
										</Button>
										<Button
											variant="ghost"
											size="sm"
											class="text-destructive hover:text-destructive"
											disabled={u.id === $currentUser?.id}
											onclick={() => deleteUser(u)}
										>
											<HugeiconsIcon icon={Delete02Icon} class="size-4" strokeWidth={2} />
										</Button>
									</div>
								</Table.Cell>
							</Table.Row>
						{/each}
					</Table.Body>
				</Table.Root>
			{/if}
		</Card.Content>
	</Card.Root>
</div>
