<script lang="ts">
  import { onMount } from 'svelte';
  import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';

  let { ready = false, onComplete }: { ready?: boolean; onComplete: () => void } = $props();
  let minTimePassed = $state(false);
  let exiting = $state(false);
  let started = $state(false);

  onMount(() => {
    let timer: ReturnType<typeof setTimeout>;
    let fallbackTimer: ReturnType<typeof setTimeout>;
    let unlisten: (() => void) | undefined;

    async function init() {
      try {
        const win = getCurrentWebviewWindow();

        // Subscribe first so we never miss a focus event
        unlisten = await win.onFocusChanged(({ payload: focused }) => {
          if (focused && !started) {
            begin();
            unlisten?.();
            unlisten = undefined;
          }
        });

        // Check if already focused (window may already be visible)
        if (await win.isFocused()) {
          if (!started) {
            begin();
            unlisten?.();
            unlisten = undefined;
          }
        }

        // Safety net: start animation even if focus event never arrives (Windows edge case)
        fallbackTimer = setTimeout(() => {
          if (!started) {
            begin();
            unlisten?.();
            unlisten = undefined;
          }
        }, 200);
      } catch {
        // Fallback for browser dev mode
        begin();
      }
    }

    function begin() {
      started = true;
      timer = setTimeout(() => { minTimePassed = true; }, 2900);
    }

    init();

    return () => {
      clearTimeout(timer);
      clearTimeout(fallbackTimer);
      unlisten?.();
    };
  });

  // Dismiss when animation minimum is met AND app data is loaded
  $effect(() => {
    if (ready && minTimePassed && !exiting) {
      exiting = true;
      setTimeout(() => onComplete(), 500);
    }
  });
</script>

<div class="splash" class:exiting>
  {#if started}
  <div class="splash-inner">
    <div class="face-wrap">
      <div class="face-alive">
        <svg viewBox="0 0 80 80" fill="none" width="76" height="76">
          <defs>
            <radialGradient id="halo">
              <stop offset="0%" stop-color="rgba(255,255,255,0.06)" />
              <stop offset="100%" stop-color="transparent" />
            </radialGradient>
          </defs>

          <!-- Soft halo -->
          <circle cx="40" cy="40" r="38" fill="url(#halo)" class="halo" />

          <!-- Solid white disc -->
          <circle class="disc" cx="40" cy="40" r="32" fill="rgba(255,255,255,0.93)" />

          <!-- Face features (grouped for morph fade-out) -->
          <g class="features">
            <!-- Left eye: dot -->
            <circle class="eye-left" cx="30" cy="33" r="5.5" fill="#111113" />

            <!-- Right eye: dot + wink chevron -->
            <g class="eye-right-group">
              <circle class="eye-right-dot" cx="50" cy="33" r="5.5" fill="#111113" />
              <path class="eye-right-wink" d="M53.5,29 L47.5,33 L53.5,37"
                stroke="#111113" stroke-width="2.8" stroke-linecap="round"
                stroke-linejoin="round" fill="none" />
            </g>
          </g>
        </svg>
      </div>
    </div>
    <span class="name">TokenMonitor</span>
  </div>
  {/if}
</div>

<style>
  .splash {
    display: flex;
    align-items: center;
    justify-content: center;
    min-height: 280px;
    padding: 60px 24px;
  }

  .splash-inner {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 16px;
  }

  /* ── Circle entrance ── */
  .face-wrap {
    opacity: 0;
    transform: scale(0.5);
    animation: wrapIn 0.45s cubic-bezier(0.34, 1.56, 0.64, 1) 0.05s both;
  }
  @keyframes wrapIn {
    to { opacity: 1; transform: scale(1); }
  }

  /* ── Organic idle motion — irregular drift/tilt ── */
  .face-alive {
    animation: alive 3s ease-in-out 0.50s infinite;
  }
  @keyframes alive {
    0%   { transform: translate(0, 0) rotate(0deg); }
    18%  { transform: translate(1.2px, -3px) rotate(2deg); }
    40%  { transform: translate(-1.8px, -1px) rotate(-1.5deg); }
    58%  { transform: translate(0.8px, -3.5px) rotate(0.5deg); }
    75%  { transform: translate(-1px, -0.5px) rotate(-2.2deg); }
    90%  { transform: translate(0.5px, -1.8px) rotate(1.2deg); }
    100% { transform: translate(0, 0) rotate(0deg); }
  }

  /* ── Theme-adaptive fills (CSS overrides SVG presentation attributes) ── */
  .disc { fill: var(--splash-disc); }
  .eye-left, .eye-right-dot { fill: var(--splash-eye); }
  .eye-right-wink { stroke: var(--splash-eye); }

  .halo {
    opacity: 0;
    animation: fadeIn 0.5s ease-out 0.15s both;
  }

  /* ── Eyes entrance ── */
  .eye-left {
    opacity: 0;
    animation: fadeIn 0.2s ease-out 0.25s both;
  }

  .eye-right-group {
    opacity: 0;
    animation: fadeIn 0.2s ease-out 0.30s both;
  }

  /* ── Wink: crossfade dot ↔ < chevron ── */
  .eye-right-dot {
    animation: winkDot 0.5s ease-in-out 1.0s both;
  }
  @keyframes winkDot {
    0%   { opacity: 1; }
    15%  { opacity: 0; }
    75%  { opacity: 0; }
    100% { opacity: 1; }
  }

  .eye-right-wink {
    opacity: 0;
    animation: winkChevron 0.5s ease-in-out 1.0s both;
  }
  @keyframes winkChevron {
    0%   { opacity: 0; }
    15%  { opacity: 1; }
    75%  { opacity: 1; }
    100% { opacity: 0; }
  }

  /* ── Morph: eyes fade → clean solid circle ── */
  .features {
    animation: featuresOut 0.35s ease-in 1.70s both;
  }
  @keyframes featuresOut {
    to { opacity: 0; }
  }

  /* ── App name ── */
  .name {
    font: 500 13px/1 'Inter', -apple-system, system-ui, sans-serif;
    color: var(--t2);
    letter-spacing: 0.5px;
    opacity: 0;
    animation: nameIn 0.4s ease-out 0.70s both;
  }

  /* ── Shared keyframes ── */
  @keyframes fadeIn {
    to { opacity: 1; }
  }
  @keyframes nameIn {
    from { opacity: 0; transform: translateY(6px); }
    to   { opacity: 1; transform: translateY(0); }
  }

  /* ── Exit ── */
  .exiting .face-alive {
    animation-play-state: paused;
  }
  .exiting .splash-inner {
    animation: exitAnim 0.45s cubic-bezier(0.2, 0, 0.4, 1) both;
  }
  @keyframes exitAnim {
    to { opacity: 0; transform: scale(0.88); }
  }
</style>
