<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { invoke } from "@tauri-apps/api/core";
  import { activeProvider, activePeriod } from "../stores/usage.js";
  import { get } from "svelte/store";

  type WarmupProgress = {
    current: number;
    total: number;
    provider: string;
    period: string;
    offset: number;
  };

  let running = $state(false);
  let progress = $state<WarmupProgress | null>(null);
  let doneCount = $state<number | null>(null);

  let unlistenProgress: (() => void) | null = null;
  let unlistenDone: (() => void) | null = null;

  function cleanup() {
    unlistenProgress?.();
    unlistenDone?.();
    unlistenProgress = null;
    unlistenDone = null;
  }

  async function startWarmup() {
    running = true;
    progress = null;
    doneCount = null;

    unlistenProgress = await listen<WarmupProgress>("cache://progress", (e) => {
      progress = e.payload;
    });
    unlistenDone = await listen<number>("cache://done", (e) => {
      running = false;
      doneCount = e.payload;
      cleanup();
    });

    try {
      await invoke("start_cache_warmup", {
        priorityProvider: get(activeProvider),
        priorityPeriod: get(activePeriod),
      });
    } catch {
      running = false;
      cleanup();
    }
  }

  async function cancelWarmup() {
    await invoke("cancel_cache_warmup").catch(() => {});
    running = false;
    progress = null;
    cleanup();
  }
</script>

<div class="warmup-block">
  {#if running && progress}
    <div class="warmup-progress">
      <div class="progress-bar">
        <div class="fill" style="width: {(progress.current / progress.total * 100).toFixed(1)}%"></div>
      </div>
      <div class="progress-text">
        {progress.current}/{progress.total}
        {#if progress.provider}
          <span class="progress-detail">{progress.provider}/{progress.period} [{progress.offset}]</span>
        {/if}
      </div>
    </div>
    <button class="cancel-btn" type="button" onclick={cancelWarmup}>Cancel</button>
  {:else if running}
    <button class="warmup-btn running" type="button" disabled>Preparing...</button>
  {:else}
    <button class="warmup-btn" type="button" onclick={startWarmup}>
      {#if doneCount !== null}
        Reloaded {doneCount}
      {:else}
        Reload
      {/if}
    </button>
  {/if}
</div>

<style>
  .warmup-block {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .warmup-progress {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .progress-bar {
    height: 4px;
    background: var(--border);
    border-radius: 2px;
    overflow: hidden;
  }

  .progress-bar .fill {
    height: 100%;
    background: var(--accent);
    border-radius: 2px;
    transition: width 0.3s ease;
  }

  .progress-text {
    font-size: 10px;
    color: var(--text-secondary);
    display: flex;
    gap: 4px;
  }

  .progress-detail {
    opacity: 0.7;
  }

  .warmup-btn,
  .cancel-btn {
    background: var(--surface-hover);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 2px 8px;
    font: 400 8px/1.2 'Inter', sans-serif;
    color: var(--t2);
    cursor: pointer;
    white-space: nowrap;
  }

  .warmup-btn:hover,
  .cancel-btn:hover {
    color: var(--t1);
    border-color: var(--t3);
  }

  .warmup-btn.running {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .cancel-btn {
    border-color: var(--error, #e55);
    color: var(--error, #e55);
  }
</style>
