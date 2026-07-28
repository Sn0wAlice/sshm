<script lang="ts">
  import { flip } from "svelte/animate";
  import { fly } from "svelte/transition";
  import { CheckCircle2, AlertCircle, Info } from "lucide-svelte";
  import { toasts, dismissToast } from "../stores";

  const icons = { ok: CheckCircle2, err: AlertCircle, info: Info };
</script>

<div class="wrap">
  {#each $toasts as t (t.id)}
    <div
      class="toast {t.kind}"
      animate:flip={{ duration: 180 }}
      in:fly={{ y: 12, duration: 180 }}
      out:fly={{ y: 12, duration: 140 }}
    >
      <svelte:component this={icons[t.kind]} size={16} class="ic" />
      <span class="txt">{t.text}</span>
      {#if t.action}
        <button
          class="act"
          on:click={() => {
            t.action?.run();
            dismissToast(t.id);
          }}>{t.action.label}</button
        >
      {/if}
      <button class="x" on:click={() => dismissToast(t.id)} aria-label="Dismiss">✕</button>
    </div>
  {/each}
</div>

<style>
  .wrap {
    position: fixed;
    right: 16px;
    bottom: 16px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    z-index: 90;
  }
  .toast {
    display: flex;
    align-items: center;
    gap: 10px;
    background: var(--bg-2);
    border: 1px solid var(--border);
    color: var(--fg);
    border-radius: 11px;
    padding: 10px 12px;
    min-width: 260px;
    max-width: 380px;
    box-shadow: 0 14px 40px rgba(0, 0, 0, 0.5);
  }
  .toast :global(.ic) {
    flex: none;
  }
  .toast.ok :global(.ic) {
    color: var(--ok);
  }
  .toast.err :global(.ic) {
    color: var(--danger);
  }
  .toast.info :global(.ic) {
    color: var(--fg-dim);
  }
  .txt {
    flex: 1;
    font-size: 13px;
    overflow-wrap: anywhere;
  }
  .act {
    background: var(--bg-3);
    border: 1px solid var(--border);
    padding: 4px 10px;
    border-radius: 7px;
    font-size: 12px;
    font-weight: 600;
    flex: none;
  }
  .x {
    background: none;
    border: none;
    color: var(--fg-faint);
    padding: 2px 4px;
    flex: none;
  }
  .x:hover {
    color: var(--fg);
  }
</style>
