# Repository Guidelines

## Project Structure & Module Organization

`src/` contains the Svelte 5 frontend. `App.svelte` is the main shell, `lib/components/` holds UI components, `lib/stores/` manages app state and IPC-backed data loading, and `lib/utils/`, `lib/window/`, `lib/tray/`, and `lib/views/` contain focused helpers. Frontend tests live beside the code as `*.test.ts`.

`src-tauri/` contains the Rust backend. `src/lib.rs` wires the app, `src/commands/` exposes Tauri commands, and modules such as `usage/`, `stats/`, `tray/`, `platform/`, and `rate_limits/` hold domain logic. Use `docs/` for design notes, `build/` for packaging scripts, and `scripts/` for release/setup helpers.

## Build, Test, and Development Commands

- `npm ci`: install frontend dependencies from `package-lock.json`.
- `npx tauri dev`: run the desktop app with the Svelte frontend and Rust backend together.
- `npm run dev`: run the Vite frontend only.
- `npm run build`: build the frontend into `dist/`.
- `npx tauri build`: create production desktop bundles.
- `npm test`: run Vitest.
- `npm run test:coverage`: generate V8 coverage in `coverage/`.
- `npm run test:all`: run Rust and TypeScript tests together.
- `npx svelte-check`, `cargo fmt --manifest-path src-tauri/Cargo.toml --check`, `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`: match the main validation checks.

## Coding Style & Naming Conventions

Use 2-space indentation in TypeScript and Svelte, 4 spaces in Rust, double quotes in TS, and semicolons enabled. Name Svelte components in PascalCase, such as `Settings.svelte`; TS modules in camelCase, such as `providerMetadata.ts`; and Rust modules in snake_case. Keep shared frontend payload types under `src/lib/types/`.

## Testing Guidelines

Use Vitest for frontend/unit tests and inline `#[cfg(test)]` modules for Rust. Add targeted tests when changing parsing, pricing, rate-limit logic, store behavior, or provider merges. Keep test files colocated with source and named `*.test.ts`.

## Commit & Pull Request Guidelines

Prefer short, imperative commit subjects. Recent history favors prefixes like `feat:`, `docs:`, `test(scope):`, and `chore(release):`. Pull requests should describe the problem and fix, list validation commands run, link related issues, and include screenshots or GIFs for UI changes.

## Configuration & Safety Notes

This app is local-first and reads logs from paths like `~/.claude/projects/**` and `~/.codex/sessions/**`. Prefer config-aware paths over hardcoded user directories. Do not commit generated outputs such as `dist/`, `coverage/`, `src-tauri/target/`, `src-tauri/gen/`, or signing artifacts.
