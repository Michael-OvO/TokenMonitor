<script lang="ts">
  import { settings } from "../stores/settings.js";
  import { isMacOS } from "../utils/platform.js";
  import { getPermissionSurfaces } from "../permissions/surfaces.js";
  import type { PermissionSurfaceId } from "../permissions/surfaces.js";

  interface Props {
    mode?: "settings";
    /** When provided (settings mode), actionable surfaces render a
     * "Manage →" link that routes the user to the section owning the real
     * control (Launch at Login → System, SSH remote devices → Visibility).
     * The disclosure panel itself stays read-only and never mutates state. */
    onManage?: (id: PermissionSurfaceId) => void;
  }

  let { mode = "settings", onManage }: Props = $props();

  /** Surfaces whose toggle lives in another Settings section. In settings
   * mode these get a "Manage →" link so the row is no longer a dead end. */
  const MANAGEABLE_SURFACES: PermissionSurfaceId[] = ["login_item", "ssh_config"];
  let surfaces = $derived.by(() => getPermissionSurfaces($settings, { macos: isMacOS() }));
</script>

<div class="permission-list permission-list-{mode}">
  {#each surfaces as surface}
    <div class="permission-row">
      <div class="permission-head">
        <span class="permission-title">{surface.title}</span>
        <span class="permission-status status-{surface.tone}">{surface.status}</span>
      </div>
      <p class="permission-copy">{surface.why}</p>
      <p class="permission-policy">{surface.requestCopy}</p>
      {#if surface.paths.length > 0}
        <div class="permission-paths">
          {#each surface.paths as path}
            <code>{path}</code>
          {/each}
        </div>
      {/if}
      {#if mode === "settings" && onManage && MANAGEABLE_SURFACES.includes(surface.id)}
        <button type="button" class="permission-manage" onclick={() => onManage?.(surface.id)}>
          Manage<span class="permission-manage-arrow" aria-hidden="true">→</span>
        </button>
      {/if}
    </div>
  {/each}
</div>

<style>
  .permission-list {
    display: flex;
    flex-direction: column;
    gap: 1px;
    border-radius: 8px;
    overflow: hidden;
    background: var(--border-subtle);
  }
  .permission-list-settings {
    border-radius: 0;
  }

  .permission-row {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 8px 10px;
    background: var(--surface-2);
    min-width: 0;
  }

  .permission-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    min-width: 0;
  }

  .permission-title {
    font: 500 10.5px/1.25 "Inter", sans-serif;
    color: var(--t1);
    min-width: 0;
  }

  .permission-status {
    flex-shrink: 0;
    font: 500 9px/1 "Inter", sans-serif;
  }

  .status-ok { color: var(--ch-plus); }
  .status-warn { color: #E8A060; }
  .status-neutral { color: var(--t4); }

  .permission-copy,
  .permission-policy {
    margin: 0;
    font: 400 9.5px/1.35 "Inter", sans-serif;
    color: var(--t3);
  }

  .permission-policy {
    color: var(--t4);
  }

  .permission-manage {
    align-self: flex-start;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    margin-top: 2px;
    padding: 0;
    background: none;
    border: none;
    cursor: pointer;
    color: var(--accent, #6366f1);
    font: 500 9.5px/1 "Inter", sans-serif;
  }
  .permission-manage:hover {
    text-decoration: underline;
  }
  .permission-manage-arrow {
    font-size: 11px;
    line-height: 1;
  }

  .permission-paths {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    min-width: 0;
  }

  .permission-paths code {
    display: inline-block;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    border-radius: 4px;
    padding: 2px 4px;
    background: var(--surface-hover);
    color: var(--t3);
    font: 400 8.5px/1.2 ui-monospace, SFMono-Regular, Menlo, monospace;
  }
</style>
