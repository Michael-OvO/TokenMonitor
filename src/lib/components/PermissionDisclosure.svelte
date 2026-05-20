<script lang="ts">
  import { settings } from "../stores/settings.js";
  import { isMacOS } from "../utils/platform.js";
  import { getPermissionSurfaces } from "../permissions/surfaces.js";

  interface Props {
    mode?: "welcome" | "settings" | "rate-limit";
  }

  let { mode = "settings" }: Props = $props();
  let surfaces = $derived.by(() => {
    const all = getPermissionSurfaces($settings, { macos: isMacOS() });
    if (mode === "rate-limit") {
      return all.filter((surface) => surface.id === "claude_statusline");
    }
    if (mode === "welcome") {
      return all.filter((surface) =>
        surface.id === "usage_logs" ||
        surface.id === "claude_statusline" ||
        surface.id === "login_item" ||
        surface.id === "updates",
      );
    }
    return all;
  });
</script>

<div class="permission-list permission-list-{mode}">
  {#each surfaces as surface}
    <div class="permission-row">
      <div class="permission-head">
        <span class="permission-title">{surface.title}</span>
        <span class="permission-status status-{surface.tone}">{surface.status}</span>
      </div>
      <p class="permission-copy">{surface.why}</p>
      {#if mode !== "welcome"}
        <p class="permission-policy">{surface.requestCopy}</p>
        {#if surface.paths.length > 0}
          <div class="permission-paths">
            {#each surface.paths as path}
              <code>{path}</code>
            {/each}
          </div>
        {/if}
      {:else}
        <p class="permission-policy compact">{surface.requestCopy}</p>
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

  .permission-list-welcome .permission-row {
    padding: 7px 9px;
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

  .permission-policy.compact {
    display: none;
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
