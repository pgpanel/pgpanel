import { writable } from 'svelte/store';
import { api, setCsrfToken, type MeResponse, type User } from './api';

export const user = writable<User | null>(null);
export const authReady = writable(false);

export async function refreshMe(): Promise<MeResponse | null> {
  try {
    const me = await api<MeResponse>('/api/me');
    setCsrfToken(me.csrf_token);
    user.set(me.user);
    return me;
  } catch {
    user.set(null);
    return null;
  } finally {
    authReady.set(true);
  }
}

export async function login(username: string, password: string) {
  const me = await api<MeResponse>('/api/auth/login', {
    method: 'POST',
    body: JSON.stringify({ username, password })
  });
  setCsrfToken(me.csrf_token);
  user.set(me.user);
  return me;
}

export async function bootstrap(
  username: string,
  password: string,
  bootstrap_token?: string
) {
  await api('/api/auth/bootstrap', {
    method: 'POST',
    body: JSON.stringify({ username, password, bootstrap_token })
  });
  return login(username, password);
}

export async function logout() {
  try {
    await api('/api/auth/logout', { method: 'POST' });
  } finally {
    setCsrfToken('');
    user.set(null);
  }
}
