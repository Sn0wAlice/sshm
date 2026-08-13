<script lang="ts">
  import { fade, scale } from "svelte/transition";
  import type { ContainerDetail } from "../bindings";
  import { klDetail } from "../kluster";

  let detail: ContainerDetail | null = null;
  let loading = false;
  let error = "";

  // Load whenever a new request comes in.
  let seq = 0;
  $: handle($klDetail);

  async function handle(req: typeof $klDetail): Promise<void> {
    const mine = ++seq;
    detail = null;
    error = "";
    if (!req) return;
    if ("detail" in req) {
      detail = req.detail;
      return;
    }
    loading = true;
    const r = await req.fetch();
    if (mine !== seq) return; // superseded by a newer request
    loading = false;
    if (r.status === "ok") detail = r.data;
    else error = r.error;
  }

  function close(): void {
    klDetail.set(null);
  }
  function onKey(e: KeyboardEvent): void {
    if (e.key === "Escape") {
      e.preventDefault();
      close();
    }
  }
</script>

<svelte:window on:keydown={onKey} />

{#if $klDetail}
  <div class="backdrop" transition:fade={{ duration: 120 }} on:click|self={close} role="presentation">
    <div class="modal" transition:scale={{ duration: 140, start: 0.97 }}>
      <div class="head">
        <div class="title mono">{$klDetail.title}</div>
        <button class="ghost" on:click={close} title="Close (Esc)">✕</button>
      </div>

      <div class="body scroll">
        {#if loading}
          <p class="muted">Inspecting…</p>
        {:else if error}
          <p class="err">{error}</p>
        {:else if detail}
          {#each detail.sections as sec}
            <div class="section">
              <div class="sec-title">{sec.title}</div>
              <div class="rows">
                {#each sec.rows as [k, v]}
                  <div class="row"><span class="k">{k}</span><span class="v mono">{v}</span></div>
                {/each}
              </div>
            </div>
          {/each}
          {#if detail.log_tail.length}
            <div class="section">
              <div class="sec-title">Recent logs</div>
              <pre class="logs">{detail.log_tail.join("\n")}</pre>
            </div>
          {/if}
          {#if detail.sections.length === 0 && detail.log_tail.length === 0}
            <p class="muted">No details reported.</p>
          {/if}
        {/if}
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
    width: min(560px, 94vw);
    max-height: 82vh;
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: 14px;
    padding: 18px 20px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    box-shadow: 0 24px 70px rgba(0, 0, 0, 0.6);
  }
  .head {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .title {
    font-size: 15px;
    font-weight: 650;
    flex: 1;
    overflow-wrap: anywhere;
  }
  .body {
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 14px;
    min-height: 0;
  }
  .mono {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  }
  .muted {
    color: var(--fg-dim);
  }
  .err {
    color: var(--danger);
  }
  .sec-title {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--fg-dim);
    margin-bottom: 6px;
  }
  .rows {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .row {
    display: flex;
    gap: 14px;
    font-size: 12.5px;
  }
  .row .k {
    flex: none;
    width: 120px;
    color: var(--fg-dim);
  }
  .row .v {
    overflow-wrap: anywhere;
    min-width: 0;
  }
  .logs {
    background: var(--bg-1);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 10px;
    font-size: 12px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    margin: 0;
    max-height: 240px;
    overflow-y: auto;
  }
  .ghost {
    background: none;
    border: none;
    color: var(--fg-dim);
    padding: 4px 8px;
    font-size: 15px;
  }
</style>
