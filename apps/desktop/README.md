# Constellation Desktop

This is the Tauri 2 + Svelte 5 desktop entrypoint for the local-first
Constellation development project.

See [the root README](../../README.md) for product scope, prerequisites,
development commands, restrictions, and native Windows GUI evidence.

The TypeScript code owns presentation only. The Rust native command boundary
in `src-tauri` owns local SQLite persistence, provider HTTP calls, and agent
task state. No unrestricted shell or arbitrary filesystem tools are exposed.
