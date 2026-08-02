import { writable, get } from 'svelte/store';

/** Session-level wizard gate so completing the wizard never loops back. */
export const wizardNeeded = writable<boolean | null>(null);

export function markWizardDone() {
	wizardNeeded.set(false);
	if (typeof sessionStorage !== 'undefined') {
		sessionStorage.setItem('pgpanel_wizard_done', '1');
	}
}

export function isWizardMarkedDone(): boolean {
	if (typeof sessionStorage !== 'undefined' && sessionStorage.getItem('pgpanel_wizard_done') === '1') {
		return true;
	}
	return get(wizardNeeded) === false;
}
