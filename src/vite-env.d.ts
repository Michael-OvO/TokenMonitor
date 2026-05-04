/// <reference types="svelte" />
/// <reference types="vite/client" />

/**
 * Build-time env vars Vite injects into the bundle. All must be prefixed
 * with `VITE_` per Vite's security contract (anything else is filtered out
 * so server secrets can't leak into the client). Declarations live here so
 * the rest of the app gets typed access via `import.meta.env.<name>`.
 */
interface ImportMetaEnv {
  /**
   * Dev-only override: when set to a truthy string, `loadSettings` forces
   * `hasSeenWelcome=false` and rewinds `lastOnboardedVersion` to a value
   * older than the current build's `CURRENT_ONBOARDING_VERSION`, so every
   * launch reopens the onboarding wizard as if the user just upgraded.
   * Auto-gated on `import.meta.env.DEV` so production builds ignore it.
   *
   * Usage:
   *   VITE_TM_FORCE_ONBOARDING=1 npm run tauri dev
   * Or persist the flag for a session by adding to `.env.local`:
   *   VITE_TM_FORCE_ONBOARDING=1
   */
  readonly VITE_TM_FORCE_ONBOARDING?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
