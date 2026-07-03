(function () {
    'use strict';

    // Theme toggle
    const themeToggle = document.querySelector('.theme-toggle');
    const html = document.documentElement;

    function setTheme(theme) {
        html.setAttribute('data-theme', theme);
        localStorage.setItem('scotia-theme', theme);
    }

    const savedTheme = localStorage.getItem('scotia-theme');
    if (savedTheme) {
        setTheme(savedTheme);
    }

    themeToggle?.addEventListener('click', () => {
        const current = html.getAttribute('data-theme') || 'dark';
        setTheme(current === 'dark' ? 'light' : 'dark');
    });

    // Glyph interactions
    const glyphDetail = document.getElementById('glyphDetail');
    const glyphDescriptions = {
        'run_started': 'A new agent invocation begins. Scotia initializes the ledger and captures metadata such as agent type, task, and timestamp.',
        'prompt_submitted': 'The user prompt or high-level instruction is recorded, forming the starting context of the decision trace.',
        'action_invoked': 'The agent calls a tool or executes a file operation. This is the primary observable unit of agent behaviour.',
        'action_result': 'The outcome of a tool call returns—success, failure, or structured data—and is appended to the ledger.',
        'model_routed': 'Execution switches between models or providers. Scotia records latency, cost, and fallback-chain metadata.',
        'response_chunk': 'A streaming response fragment is captured, preserving the shape of the agent output without full transcription.',
        'error_or_retry': 'An error, refusal, or automatic retry occurs. These events are critical for debugging and regression tests.',
        'state_delta': 'A change to files, memory, or session state is observed and stored as a diffable record.',
        'run_finished': 'The invocation terminates. Scotia finalizes the JSON event log, Markdown summary, and DOT graph.'
    };

    document.querySelectorAll('.glyph').forEach(glyph => {
        const name = glyph.getAttribute('data-glyph');
        const symbol = glyph.querySelector('.glyph-symbol')?.textContent || '';

        function activate() {
            document.querySelectorAll('.glyph').forEach(g => g.classList.remove('active'));
            glyph.classList.add('active');
            if (glyphDetail) {
                glyphDetail.innerHTML = `<strong>${symbol} ${name}</strong><p>${glyphDescriptions[name] || ''}</p>`;
            }
        }

        glyph.addEventListener('mouseenter', activate);
        glyph.addEventListener('focus', activate);
    });

    // Copy buttons
    document.querySelectorAll('.copy-btn').forEach(btn => {
        btn.addEventListener('click', async () => {
            const block = btn.closest('.code-block');
            const code = block?.querySelector('code');
            const text = btn.getAttribute('data-copy') || code?.textContent || '';
            try {
                await navigator.clipboard.writeText(text);
                const original = btn.textContent;
                btn.textContent = 'Copied';
                btn.classList.add('copied');
                setTimeout(() => {
                    btn.textContent = original;
                    btn.classList.remove('copied');
                }, 1500);
            } catch (err) {
                console.error('Copy failed', err);
            }
        });
    });

    // Puzzlebox speed boost on click
    const puzzlebox = document.getElementById('puzzlebox');
    puzzlebox?.addEventListener('click', () => {
        puzzlebox.style.animationDuration = '1s';
        setTimeout(() => {
            puzzlebox.style.animationDuration = '';
        }, 2000);
    });

    // Reveal animations on scroll
    const observer = new IntersectionObserver((entries) => {
        entries.forEach(entry => {
            if (entry.isIntersecting) {
                entry.target.style.opacity = '1';
                entry.target.style.transform = 'translateY(0)';
            }
        });
    }, { threshold: 0.1 });

    document.querySelectorAll('.mechanism-card, .feature, .agent-card, .download-card, .step').forEach(el => {
        el.style.opacity = '0';
        el.style.transform = 'translateY(20px)';
        el.style.transition = 'opacity 0.6s ease, transform 0.6s ease';
        observer.observe(el);
    });
})();
