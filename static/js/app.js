// Walle App JavaScript

// HTMX configuration
document.addEventListener('htmx:configRequest', (event) => {
    // Add content type for form submissions
    if (event.detail.verb !== 'get') {
        event.detail.headers['Content-Type'] = 'application/x-www-form-urlencoded';
    }
});

document.addEventListener('htmx:afterSwap', (event) => {
    // Re-initialize Alpine on swapped content if needed
});

// Handle successful responses
document.addEventListener('htmx:afterRequest', (event) => {
    if (event.detail.successful) {
        // Clear form after successful POST/PUT
        if (event.detail.verb === 'post' || event.detail.verb === 'put') {
            const form = event.detail.elt;
            if (form.tagName === 'FORM' && !form.hasAttribute('hx-preserve')) {
                form.reset();
            }
        }
    }
});

// Handle server errors
document.addEventListener('htmx:responseError', (event) => {
    console.error('HTMX Error:', event.detail);
    showToast('Something went wrong. Please try again.', 'error');
});

// Handle connection errors
document.addEventListener('htmx:sendError', (event) => {
    console.error('HTMX Send Error:', event.detail);
    showToast('Connection error. Please check your network.', 'error');
});

// Confirmation dialogs
document.addEventListener('htmx:confirm', (event) => {
    // Built-in hx-confirm handling is sufficient
});

// Toast notifications
window.showToast = function(message, type = 'info') {
    const toast = document.createElement('div');
    toast.className = `fixed bottom-4 right-4 px-6 py-3 rounded shadow-lg text-white z-50 animate-fade-in ${
        type === 'success' ? 'bg-emerald-600' :
        type === 'error' ? 'bg-red-600' :
        'bg-surface-800 border border-surface-700'
    }`;
    toast.textContent = message;
    document.body.appendChild(toast);
    
    setTimeout(() => {
        toast.classList.add('animate-fade-out');
        setTimeout(() => toast.remove(), 150);
    }, 3000);
};

// Utility: Format dates
function formatDate(dateStr) {
    if (!dateStr) return '—';
    return new Date(dateStr).toLocaleString();
}

// Utility: Format duration
function formatDuration(seconds) {
    if (!seconds) return '—';
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    return `${mins}m ${secs}s`;
}
