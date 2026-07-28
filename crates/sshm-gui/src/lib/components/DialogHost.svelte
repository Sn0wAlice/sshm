<script lang="ts">
  import { fade, scale } from "svelte/transition";
  import { activeDialog, type DialogRequest } from "../dialogs";

  let inputValue = "";

  // Reset the input each time a new prompt opens.
  let lastRef: DialogRequest | null = null;
  $: if ($activeDialog !== lastRef) {
    lastRef = $activeDialog;
    inputValue = $activeDialog?.kind === "prompt" ? ($activeDialog.opts.initial ?? "") : "";
  }

  function close(): void {
    activeDialog.set(null);
  }
  function cancel(): void {
    const d = $activeDialog;
    if (!d) return;
    if (d.kind === "confirm") d.resolve(false);
    else d.resolve(null);
    close();
  }
  function accept(): void {
    const d = $activeDialog;
    if (!d) return;
    if (d.kind === "confirm") d.resolve(true);
    else d.resolve(inputValue.trim() ? inputValue.trim() : null);
    close();
  }
  function onKey(e: KeyboardEvent): void {
    if (e.key === "Escape") {
      e.preventDefault();
      cancel();
    } else if (e.key === "Enter" && $activeDialog?.kind === "prompt") {
      e.preventDefault();
      accept();
    }
  }

  function autofocus(node: HTMLInputElement): void {
    requestAnimationFrame(() => node.focus());
  }
</script>

<svelte:window on:keydown={onKey} />

{#if $activeDialog}
  {@const d = $activeDialog}
  <div class="backdrop" transition:fade={{ duration: 120 }} on:click|self={cancel} role="presentation">
    <div class="modal" transition:scale={{ duration: 140, start: 0.97 }}>
      <div class="title">{d.opts.title}</div>
      {#if d.opts.message}<div class="msg">{d.opts.message}</div>{/if}

      {#if d.kind === "prompt"}
        <input
          use:autofocus
          bind:value={inputValue}
          placeholder={d.opts.placeholder ?? ""}
        />
      {/if}

      <div class="actions">
        <button on:click={cancel}>{d.kind === "confirm" ? (d.opts.cancelLabel ?? "Cancel") : "Cancel"}</button>
        <button
          class:primary={!(d.kind === "confirm" && d.opts.danger)}
          class:danger={d.kind === "confirm" && d.opts.danger}
          on:click={accept}
        >
          {d.kind === "confirm" ? (d.opts.confirmLabel ?? "Confirm") : (d.opts.confirmLabel ?? "OK")}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.55);
    backdrop-filter: blur(2px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 80;
  }
  .modal {
    width: min(420px, 92vw);
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: 14px;
    padding: 20px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    box-shadow: 0 24px 70px rgba(0, 0, 0, 0.6);
  }
  .title {
    font-size: 15px;
    font-weight: 650;
  }
  .msg {
    color: var(--fg-dim);
    font-size: 13px;
    line-height: 1.5;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 4px;
  }
  button.danger {
    background: var(--danger);
    border-color: var(--danger);
    color: #fff;
  }
  button.danger:hover {
    filter: brightness(1.08);
    background: var(--danger);
  }
</style>
