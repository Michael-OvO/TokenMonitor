<script lang="ts">
  import keychainDemo from "../assets/keychain-prompt-demo.mp4";

  /**
   * Inline preview of the macOS Keychain ACL sheet that appears when
   * TokenMonitor reads `Claude Code-credentials`. Plays a short looping
   * screen-capture of the real prompt so the user sees, in context, exactly
   * what the system sheet will look like and which button ("Always Allow")
   * to click. The video is muted, autoplays, loops, and has no controls so
   * it reads as a passive demo, not a media player.
   *
   * No system call happens here — the parent's `onContinue` is what actually
   * triggers the real prompt via `requestClaudeKeychainAccessAgain`.
   */
  interface Props {
    busy?: boolean;
    onContinue: () => void;
    onCancel: () => void;
  }

  let { busy = false, onContinue, onCancel }: Props = $props();
</script>

<div class="kp-wrap" role="group" aria-labelledby="kp-hint">
  <div class="kp-frame">
    <video
      class="kp-video"
      src={keychainDemo}
      autoplay
      muted
      loop
      playsinline
      preload="auto"
      aria-hidden="true"
    ></video>
  </div>

  <div class="kp-foot">
    <span class="kp-hint" id="kp-hint">
      Click <strong>Always Allow</strong> when the real macOS prompt appears.
    </span>
    <div class="kp-foot-actions">
      <button type="button" class="kp-cta-secondary" onclick={onCancel} disabled={busy}>
        Cancel
      </button>
      <button type="button" class="kp-cta-primary" onclick={onContinue} disabled={busy}>
        {busy ? "Opening prompt…" : "Continue"}
      </button>
    </div>
  </div>
</div>

<style>
  .kp-wrap {
    margin: 8px 14px 6px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    animation: kpRise var(--t-slow, 320ms) var(--ease-spring, cubic-bezier(0.34,1.4,0.64,1)) both;
  }
  @keyframes kpRise {
    from { transform: translateY(4px); opacity: 0; }
    to   { transform: translateY(0);   opacity: 1; }
  }

  /* Frame around the video — subtle border + soft shadow so the captured
     dialog reads as embedded UI, not a media file. We let the video carry
     all of the macOS dialog styling itself. */
  .kp-frame {
    border-radius: 12px;
    overflow: hidden;
    background: transparent;
    box-shadow:
      0 0 0 0.5px rgba(255,255,255,0.06),
      0 8px 24px rgba(0,0,0,0.42);
    position: relative;
  }
  :global([data-theme="light"]) .kp-frame {
    box-shadow:
      0 0 0 0.5px rgba(0,0,0,0.10),
      0 8px 22px rgba(0,0,0,0.12);
  }

  .kp-video {
    display: block;
    width: 100%;
    height: auto;
    /* Crisp playback; no controls/UI chrome. */
    object-fit: cover;
    pointer-events: none;
  }

  /* Footer — instruction + real action buttons. */
  .kp-foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 0 2px;
  }
  .kp-hint {
    font: 400 9.5px/1.45 'Inter', sans-serif;
    color: var(--t3);
    flex: 1;
    min-width: 0;
  }
  .kp-foot-actions {
    display: flex;
    gap: 6px;
    flex-shrink: 0;
  }
  .kp-cta-secondary,
  .kp-cta-primary {
    appearance: none;
    border: none;
    cursor: pointer;
    border-radius: 6px;
    padding: 5px 10px;
    font: 500 10px/1 'Inter', sans-serif;
    transition: background var(--t-fast, 120ms) ease, transform var(--t-fast, 120ms) ease;
  }
  .kp-cta-secondary {
    background: transparent;
    color: var(--t2);
  }
  .kp-cta-secondary:hover:not(:disabled) {
    background: var(--surface-hover);
    color: var(--t1);
  }
  .kp-cta-primary {
    background: var(--accent);
    color: white;
  }
  .kp-cta-primary:hover:not(:disabled) {
    transform: translateY(-1px);
    filter: brightness(1.05);
  }
  .kp-cta-primary:disabled,
  .kp-cta-secondary:disabled {
    opacity: 0.55;
    cursor: default;
  }
</style>
