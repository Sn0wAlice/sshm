<script lang="ts">
  import { toasts } from "../stores";
</script>

<div class="wrap">
  {#each $toasts as t (t.id)}
    <div class="toast" class:err={t.kind === "err"}>{t.text}</div>
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
    z-index: 50;
  }
  .toast {
    background: var(--bg-3);
    border: 1px solid var(--ok);
    color: var(--fg);
    border-radius: var(--radius);
    padding: 8px 14px;
    max-width: 360px;
    box-shadow: 0 6px 20px rgba(0, 0, 0, 0.3);
  }
  .toast.err {
    border-color: var(--danger);
  }
</style>
