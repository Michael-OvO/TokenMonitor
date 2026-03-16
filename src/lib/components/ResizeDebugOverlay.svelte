<script lang="ts">
  import {
    clearResizeDebug,
    copyResizeDebugToClipboard,
    resizeDebugState,
    setResizeDebugEnabled,
    toggleResizeDebug,
  } from "../resizeDebug.js";

  let copied = $state(false);

  async function handleCopy() {
    try {
      await copyResizeDebugToClipboard();
      copied = true;
      setTimeout(() => {
        copied = false;
      }, 1200);
    } catch {
      copied = false;
    }
  }
</script>

{#if $resizeDebugState.enabled}
  <aside class="debug-overlay">
    <div class="toolbar">
      <div class="title">Runtime Debug</div>
      <button class="action" onclick={handleCopy}>{copied ? "Copied" : "Copy"}</button>
      <button class="action" onclick={clearResizeDebug}>Clear</button>
      <button class="action" onclick={toggleResizeDebug}>Hide</button>
    </div>

    {#if $resizeDebugState.snapshot}
      <div class="snapshot">
        <div><span>Reason</span><strong>{$resizeDebugState.snapshot.reason}</strong></div>
        <div><span>Win</span><strong>{$resizeDebugState.snapshot.windowInnerHeight}px</strong></div>
        <div><span>Viewport</span><strong>{$resizeDebugState.snapshot.visualViewportHeight ?? "n/a"}px</strong></div>
        <div><span>Last</span><strong>{$resizeDebugState.snapshot.lastWindowH}px</strong></div>
        <div><span>Max</span><strong>{$resizeDebugState.snapshot.maxWindowH}px</strong></div>
        <div><span>Pop Scroll</span><strong>{$resizeDebugState.snapshot.popScrollHeight ?? "n/a"}px</strong></div>
        <div><span>Pop Client</span><strong>{$resizeDebugState.snapshot.popClientHeight ?? "n/a"}px</strong></div>
        <div><span>Pop Offset</span><strong>{$resizeDebugState.snapshot.popOffsetHeight ?? "n/a"}px</strong></div>
        <div><span>Body Scroll</span><strong>{$resizeDebugState.snapshot.bodyScrollHeight ?? "n/a"}px</strong></div>
        <div><span>Doc Client</span><strong>{$resizeDebugState.snapshot.docClientHeight ?? "n/a"}px</strong></div>
      </div>
    {/if}

    <div class="events">
      {#each [...$resizeDebugState.events].reverse() as event (event.id)}
        <div class="event">
          <div class="event-head">
            <span class="time">{event.elapsedMs.toFixed(1)}ms</span>
            <span class="type">{event.type}</span>
          </div>
          {#if Object.keys(event.details).length > 0}
            <pre>{JSON.stringify(event.details, null, 2)}</pre>
          {/if}
        </div>
      {/each}
    </div>
  </aside>
{:else}
  <button
    class="debug-launcher"
    type="button"
    onclick={() => setResizeDebugEnabled(true)}
  >
    Debug
  </button>
{/if}

<style>
  .debug-overlay {
    position: fixed;
    right: 8px;
    bottom: 8px;
    width: min(320px, calc(100vw - 16px));
    max-height: min(380px, calc(100vh - 16px));
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px;
    border-radius: 12px;
    background: rgba(9, 9, 11, 0.92);
    border: 1px solid rgba(255,255,255,0.12);
    box-shadow: 0 10px 24px rgba(0,0,0,0.35);
    backdrop-filter: blur(14px);
    z-index: 10000;
    color: rgba(255,255,255,0.9);
    font: 500 11px/1.35 ui-monospace, "SF Mono", Menlo, monospace;
    pointer-events: auto;
  }

  .debug-launcher {
    position: fixed;
    right: 8px;
    bottom: 8px;
    z-index: 10000;
    border: 1px solid rgba(255,255,255,0.14);
    background: rgba(9, 9, 11, 0.82);
    color: rgba(255,255,255,0.82);
    border-radius: 999px;
    padding: 6px 10px;
    cursor: pointer;
    backdrop-filter: blur(12px);
    box-shadow: 0 6px 18px rgba(0,0,0,0.25);
    font: 600 11px/1 ui-monospace, "SF Mono", Menlo, monospace;
  }

  .toolbar {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .title {
    flex: 1;
    font-weight: 700;
    letter-spacing: 0.02em;
  }

  .action {
    border: 1px solid rgba(255,255,255,0.14);
    background: rgba(255,255,255,0.08);
    color: inherit;
    border-radius: 8px;
    padding: 4px 7px;
    cursor: pointer;
    font: inherit;
  }

  .snapshot {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 6px 10px;
    padding: 8px;
    border-radius: 8px;
    background: rgba(255,255,255,0.04);
  }

  .snapshot div {
    display: flex;
    justify-content: space-between;
    gap: 10px;
  }

  .snapshot span {
    color: rgba(255,255,255,0.55);
  }

  .snapshot strong {
    font-weight: 600;
  }

  .events {
    overflow: auto;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .event {
    padding: 7px 8px;
    border-radius: 8px;
    background: rgba(255,255,255,0.04);
  }

  .event-head {
    display: flex;
    gap: 8px;
    align-items: baseline;
  }

  .time {
    color: rgba(255,255,255,0.52);
    min-width: 54px;
  }

  .type {
    font-weight: 700;
  }

  pre {
    margin-top: 5px;
    white-space: pre-wrap;
    word-break: break-word;
    color: rgba(255,255,255,0.78);
  }
</style>
