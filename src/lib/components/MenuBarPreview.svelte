<script lang="ts">
  import { formatTrayTitle } from "../trayTitle.js";
  import type { TrayConfig, RateLimitsPayload } from "../types/index.js";

  interface Props {
    config: TrayConfig;
    rateLimits: RateLimitsPayload | null;
    totalCost: number;
  }

  let { config, rateLimits, totalCost }: Props = $props();

  let titleText = $derived(formatTrayTitle(config, rateLimits, totalCost));

  let claudeUtil = $derived(
    rateLimits?.claude?.windows?.[0]
      ? Math.min(rateLimits.claude.windows[0].utilization, 100)
      : 0,
  );
  let codexUtil = $derived(
    rateLimits?.codex?.windows?.[0]
      ? Math.min(rateLimits.codex.windows[0].utilization, 100)
      : 0,
  );
  let singleUtil = $derived(
    config.barProvider === 'claude' ? claudeUtil : codexUtil,
  );
</script>

<div class="mbp-wrap">
  <div class="mbp-badge">LIVE PREVIEW</div>
  <div class="mbp-bg">
    <div class="mbp-bar">
      <!-- Left context icons (faded) -->
      <div class="mbp-ctx left">
        <!-- Battery icon -->
        <svg width="18" height="10" viewBox="0 0 18 10" fill="none" class="mbp-ctx-icon">
          <rect x="0.5" y="0.5" width="14.5" height="9" rx="1.5" stroke="rgba(255,255,255,0.35)" stroke-width="0.8" fill="none"/>
          <rect x="2" y="2" width="8" height="6" rx="0.5" fill="rgba(255,255,255,0.35)"/>
          <path d="M16 3.5v3a1 1 0 0 0 0-3z" fill="rgba(255,255,255,0.35)"/>
        </svg>
        <!-- Generic app icon (circle) -->
        <svg width="12" height="12" viewBox="0 0 12 12" fill="none" class="mbp-ctx-icon">
          <circle cx="6" cy="6" r="5" stroke="rgba(255,255,255,0.35)" stroke-width="0.8" fill="none"/>
          <circle cx="6" cy="6" r="2" fill="rgba(255,255,255,0.35)"/>
        </svg>
      </div>

      <!-- CENTER: TokenMonitor widget -->
      <div class="mbp-widget">
        <!-- App icon -->
        <svg width="18" height="18" viewBox="0 0 18 18" fill="none" class="mbp-app-icon">
          <rect x="1.5" y="2.5" width="15" height="11" rx="2" stroke="rgba(255,255,255,0.82)" stroke-width="1.1" fill="none"/>
          <line x1="5" y1="15.5" x2="13" y2="15.5" stroke="rgba(255,255,255,0.82)" stroke-width="1.1" stroke-linecap="round"/>
          <line x1="9" y1="13.5" x2="9" y2="15.5" stroke="rgba(255,255,255,0.82)" stroke-width="1.1"/>
          <text x="5.5" y="10.5" fill="rgba(255,255,255,0.82)" font-size="6" font-family="monospace" font-weight="600">&gt;_</text>
        </svg>

        <!-- Bars -->
        {#if config.barDisplay === 'both'}
          <div class="mbp-bars both">
            <div class="mbp-bar-track both-track">
              <div class="mbp-bar-fill claude" style="width: {claudeUtil}%"></div>
            </div>
            <div class="mbp-bar-track both-track">
              <div class="mbp-bar-fill codex" style="width: {codexUtil}%"></div>
            </div>
          </div>
        {:else if config.barDisplay === 'single'}
          <div class="mbp-bars single">
            <div class="mbp-bar-track single-track">
              <div class="mbp-bar-fill {config.barProvider}" style="width: {singleUtil}%"></div>
            </div>
          </div>
        {/if}

        <!-- Text -->
        {#if titleText}
          <span class="mbp-text">{titleText}</span>
        {/if}
      </div>

      <!-- Right context icons (faded) -->
      <div class="mbp-ctx right">
        <!-- Wi-Fi icon -->
        <svg width="13" height="11" viewBox="0 0 13 11" fill="none" class="mbp-ctx-icon">
          <path d="M0.5 2.5C3.5 0 9.5 0 12.5 2.5" stroke="rgba(255,255,255,0.35)" stroke-width="1" stroke-linecap="round" fill="none"/>
          <path d="M2.5 4.8C4.5 3 8.5 3 10.5 4.8" stroke="rgba(255,255,255,0.35)" stroke-width="1" stroke-linecap="round" fill="none"/>
          <path d="M4.5 7C5.5 6.2 7.5 6.2 8.5 7" stroke="rgba(255,255,255,0.35)" stroke-width="1" stroke-linecap="round" fill="none"/>
          <circle cx="6.5" cy="9.5" r="1" fill="rgba(255,255,255,0.35)"/>
        </svg>
        <span class="mbp-clock">4:06 PM</span>
      </div>
    </div>
  </div>
</div>

<style>
  .mbp-wrap {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    margin-bottom: 4px;
  }

  .mbp-badge {
    font: 600 9px/1 'Inter', sans-serif;
    text-transform: uppercase;
    letter-spacing: 1.4px;
    color: rgba(255, 255, 255, 0.18);
  }

  .mbp-bg {
    width: 100%;
    padding: 14px 10px;
    border-radius: 10px;
    background:
      radial-gradient(ellipse 120% 90% at 20% 80%, #2d1b69 0%, transparent 60%),
      radial-gradient(ellipse 100% 80% at 80% 20%, #1a2a6c 0%, transparent 55%),
      radial-gradient(ellipse 140% 100% at 50% 110%, #1c0f3a 0%, transparent 50%),
      linear-gradient(135deg, #0f0c29 0%, #1b1145 40%, #24243e 100%);
  }

  .mbp-bar {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 10px;
    height: 38px;
    background: rgba(28, 28, 30, 0.82);
    backdrop-filter: saturate(150%) blur(40px);
    -webkit-backdrop-filter: saturate(150%) blur(40px);
    border-radius: 8px;
    box-shadow: 0 0 0 0.5px rgba(255, 255, 255, 0.06), 0 2px 12px rgba(0, 0, 0, 0.4);
    padding: 0 14px;
    transform: scale(1);
  }

  .mbp-ctx {
    display: flex;
    align-items: center;
    gap: 7px;
  }
  .mbp-ctx.left {
    margin-right: auto;
  }
  .mbp-ctx.right {
    margin-left: auto;
  }
  .mbp-ctx-icon {
    opacity: 1;
    flex-shrink: 0;
  }

  .mbp-clock {
    font: 400 12px/1 'Inter', -apple-system, sans-serif;
    color: rgba(255, 255, 255, 0.35);
    letter-spacing: -0.1px;
    white-space: nowrap;
  }

  .mbp-widget {
    display: flex;
    align-items: center;
    gap: 5px;
    flex-shrink: 0;
  }

  .mbp-app-icon {
    flex-shrink: 0;
    opacity: 0.82;
  }

  .mbp-bars {
    display: flex;
    flex-direction: column;
    justify-content: center;
    flex-shrink: 0;
  }
  .mbp-bars.both {
    gap: 3px;
  }

  .mbp-bar-track {
    background: rgba(255, 255, 255, 0.12);
    border-radius: 2px;
    overflow: hidden;
  }
  .mbp-bar-track.both-track {
    width: 52px;
    height: 3.5px;
  }
  .mbp-bar-track.single-track {
    width: 58px;
    height: 5px;
  }

  .mbp-bar-fill {
    height: 100%;
    border-radius: 2px;
    transition: width 0.4s cubic-bezier(0.25, 0.8, 0.25, 1);
  }
  .mbp-bar-fill.claude {
    background: #d4a574;
  }
  .mbp-bar-fill.codex {
    background: #7aafff;
  }

  .mbp-text {
    font: 400 15.5px/1 'Inter', -apple-system, sans-serif;
    font-variant-numeric: tabular-nums;
    letter-spacing: -0.15px;
    color: rgba(255, 255, 255, 0.86);
    white-space: nowrap;
    flex-shrink: 0;
  }
</style>
