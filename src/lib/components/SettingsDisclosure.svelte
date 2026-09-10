<script lang="ts">
  import type { Snippet } from "svelte";
  import { disclosureMotion } from "../utils/disclosureMotion.js";

  let { open, children }: { open: boolean; children: Snippet } = $props();
</script>

<div class="settings-disclosure" use:disclosureMotion={open} inert={!open} aria-hidden={!open}>
  <div class="disclosure-content">
    {@render children()}
  </div>
</div>

<style>
  .settings-disclosure {
    height: 0;
    overflow: hidden;
  }
  .disclosure-content {
    display: flow-root;
    contain: layout;
  }
</style>
