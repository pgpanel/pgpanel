window.pgpanelApp = function () {
  return {
    init() {
      Alpine.store('theme').init();
    },
  };
};

document.addEventListener('alpine:init', () => {
  Alpine.store('theme', {
    mode: localStorage.getItem('pgpanel-theme') || 'system',
    isDark: false,
    init() {
      this.apply();
    },
    apply() {
      const dark = this.mode === 'dark' || (this.mode === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches);
      document.documentElement.classList.toggle('dark', dark);
      this.isDark = dark;
    },
    toggle() {
      this.mode = document.documentElement.classList.contains('dark') ? 'light' : 'dark';
      localStorage.setItem('pgpanel-theme', this.mode);
      this.apply();
    },
  });

  Alpine.store('palette', {
    open: false,
    query: '',
  });

  document.addEventListener('keydown', (e) => {
    if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
      e.preventDefault();
      Alpine.store('palette').open = true;
    }
  });
});

function showToast(message, level = 'info') {
  const el = document.createElement('div');
  el.className = `fixed bottom-4 right-4 z-50 px-4 py-3 rounded-lg shadow-lg text-sm text-white ${level === 'error' ? 'bg-red-600' : 'bg-teal-600'}`;
  el.textContent = message;
  document.body.appendChild(el);
  setTimeout(() => el.remove(), 4000);
}

document.addEventListener('DOMContentLoaded', () => {
  document.querySelectorAll('form').forEach((form) => {
    form.addEventListener('htmx:configRequest', (e) => {
      const csrf = form.querySelector('input[name="csrf_token"]');
      if (csrf) e.detail.headers['HX-CSRF-Token'] = csrf.value;
    });
  });
});
