// Walle App JavaScript

// HTMX configuration
document.addEventListener('htmx:configRequest', (event) => {
    // Add CSRF token to all requests if needed
});

document.addEventListener('htmx:afterSwap', (event) => {
    // Re-initialize any components after HTMX swaps
});

// Handle form errors
document.addEventListener('htmx:responseError', (event) => {
    console.error('HTMX Error:', event.detail);
});

// Utility functions
function formatDate(dateStr) {
    if (!dateStr) return '-';
    return new Date(dateStr).toLocaleString();
}

function formatDuration(seconds) {
    if (!seconds) return '-';
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    return `${mins}m ${secs}s`;
}

// Toast notifications
window.showToast = function(message, type = 'info') {
    const toast = document.createElement('div');
    toast.className = `fixed bottom-4 right-4 px-6 py-3 rounded-lg shadow-lg text-white z-50 animate-fade-in ${
        type === 'success' ? 'bg-emerald-600' :
        type === 'error' ? 'bg-red-600' :
        'bg-slate-800'
    }`;
    toast.textContent = message;
    document.body.appendChild(toast);
    
    setTimeout(() => {
        toast.classList.add('animate-fade-out');
        setTimeout(() => toast.remove(), 300);
    }, 3000);
};

