# Why `tauri.conf.json` keeps `security.csp = null`

> `csp: null` is intentional. Tauri 2 enforces the security boundary through
> the IPC capability layer (see `src-tauri/capabilities/default.json`). Webview
> JS can only call commands / events listed in that capability file; CSP adds
> defense-in-depth only when loading remote content (we don't). Tightening
> `csp` to a strict `default-src 'self'` would also require opting the Vite
> dev URL into the allowlist — easy to get wrong.

JSON (canonical tool. `tauri.conf.json`) does not support comments, so this
rationale lives here as a sibling doc rather than as an inline note. If you
ever do need to set a real CSP, update both this file and the capability
allowlist together.