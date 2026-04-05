<script lang="ts">
  interface Props {
    checked: boolean;
    color?: string;
    onChange: (checked: boolean) => void;
  }

  let { checked, color = "#34C759", onChange }: Props = $props();
</script>

<button
  class="toggle"
  class:on={checked}
  style:--toggle-color={color}
  onclick={() => onChange(!checked)}
  role="switch"
  aria-checked={checked}
  aria-label="Toggle"
>
  <div class="knob"></div>
</button>

<style>
  .toggle {
    width: 32px;
    height: 20px;
    border-radius: 10px;
    background: rgba(120, 120, 128, 0.32);
    border: none;
    cursor: pointer;
    position: relative;
    transition: background var(--t-normal) ease, box-shadow var(--t-normal) ease;
    flex-shrink: 0;
  }
  .toggle.on {
    background: var(--toggle-color);
  }
  .knob {
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: #fff;
    position: absolute;
    top: 2px;
    left: 2px;
    transition: transform var(--t-normal) var(--ease-spring),
                background var(--t-normal) ease,
                box-shadow var(--t-normal) ease;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.2);
  }
  .toggle.on .knob {
    transform: translateX(12px);
  }

  /* ── Liquid glass treatment ── */
  :global(:root[data-glass="true"]) .toggle {
    background: rgba(255, 255, 255, 0.10);
    border: none;
    box-shadow: inset 0 0.5px 0 rgba(255, 255, 255, 0.15);
    backdrop-filter: blur(20px) saturate(1.8);
    -webkit-backdrop-filter: blur(20px) saturate(1.8);
  }
  :global(:root[data-glass="true"]) .toggle.on {
    background: color-mix(in srgb, var(--toggle-color) 60%, transparent);
    box-shadow: inset 0 0.5px 0 rgba(255, 255, 255, 0.2);
  }
  :global(:root[data-glass="true"]) .knob {
    background: rgba(255, 255, 255, 0.85);
    box-shadow: 0 0.5px 2px rgba(0, 0, 0, 0.15);
  }

  /* Light mode glass */
  :global(:root[data-glass="true"][data-theme="light"]) .toggle {
    background: rgba(0, 0, 0, 0.06);
    box-shadow: inset 0 0.5px 0 rgba(255, 255, 255, 0.5);
  }
  :global(:root[data-glass="true"][data-theme="light"]) .toggle.on {
    background: color-mix(in srgb, var(--toggle-color) 55%, transparent);
  }
</style>
