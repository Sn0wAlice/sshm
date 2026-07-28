<script lang="ts">
  import { createEventDispatcher } from "svelte";
  export let checked = false;
  const dispatch = createEventDispatcher<{ change: boolean }>();

  function toggle(): void {
    checked = !checked;
    dispatch("change", checked);
  }
</script>

<button
  type="button"
  class="switch"
  class:on={checked}
  role="switch"
  aria-checked={checked}
  on:click={toggle}
>
  <span class="knob"></span>
</button>

<style>
  .switch {
    width: 38px;
    height: 22px;
    border-radius: 999px;
    background: var(--bg-4);
    border: 1px solid var(--border);
    padding: 0;
    position: relative;
    flex: none;
    transition: background 0.15s ease, border-color 0.15s ease;
  }
  .switch.on {
    background: var(--ok);
    border-color: var(--ok);
  }
  .knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: #fff;
    transition: transform 0.16s cubic-bezier(0.3, 0.8, 0.4, 1);
  }
  .switch.on .knob {
    transform: translateX(16px);
  }
</style>
