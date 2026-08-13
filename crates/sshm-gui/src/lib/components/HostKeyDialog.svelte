<script lang="ts">
  import { fade, scale } from "svelte/transition";
  import { commands, type HostKeyInfo, type HostKeyStatus } from "../ipc";
  import { hostKeyTarget, pushToast } from "../stores";
  import Icon from "./Icon.svelte";

  let info: HostKeyInfo | null = null;
  let loading = false;
  let busy = false;
  let error = "";

  // (Re)load whenever a host is targeted; clear when the dialog closes.
  let current: string | null = null;
  $: if ($hostKeyTarget !== current) {
    current = $hostKeyTarget;
    if (current) load(current);
    else {
      info = null;
      error = "";
    }
  }

  async function load(name: string): Promise<void> {
    loading = true;
    error = "";
    info = null;
    const r = await commands.hostKeyInfo(name);
    // A late reply for a host that's no longer targeted must not clobber the UI.
    if (current !== name) return;
    loading = false;
    if (r.status === "ok") info = r.data;
    else error = r.error;
  }

  function close(): void {
    hostKeyTarget.set(null);
  }

  async function act(
    op: (name: string) => Promise<{ status: "ok"; data: null } | { status: "error"; error: string }>,
    okMsg: string,
  ): Promise<void> {
    if (!current) return;
    busy = true;
    const r = await op(current);
    busy = false;
    if (r.status === "error") {
      pushToast("err", r.error);
      return;
    }
    pushToast("ok", okMsg);
    await load(current); // reflect the new pinned/verdict state
  }

  const pin = () => act(commands.pinHostKey, "Host key pinned");
  const forget = () => act(commands.forgetHostKey, "Host key forgotten");
  const replace = () => act(commands.replaceHostKey, "Stale key replaced");

  const VERDICT: Record<HostKeyStatus, { icon: string; cls: string; text: string }> = {
    Unpinned: { icon: "?", cls: "warn", text: "Not pinned yet — trusting connects for the first time." },
    Unreachable: { icon: "…", cls: "muted", text: "Host unreachable — showing the pinned key only." },
    Match: { icon: "✓", cls: "ok", text: "The pinned key matches the server." },
    Changed: { icon: "✗", cls: "bad", text: "CHANGED — the pinned key does NOT match the server!" },
    Unknown: { icon: "?", cls: "muted", text: "No key on either side." },
  };

  function onKey(e: KeyboardEvent): void {
    if (e.key === "Escape") {
      e.preventDefault();
      close();
    }
  }
</script>

<svelte:window on:keydown={onKey} />

{#if $hostKeyTarget}
  <div class="backdrop" transition:fade={{ duration: 120 }} on:click|self={close} role="presentation">
    <div class="modal" transition:scale={{ duration: 140, start: 0.97 }}>
      <div class="title">
        <Icon name="shield" size={17} />
        <span>Host key — {$hostKeyTarget}</span>
      </div>

      {#if loading}
        <div class="msg">Checking {info?.host ?? "host"}… (scanning the server)</div>
      {:else if error}
        <div class="msg bad">{error}</div>
      {:else if info}
        <div class="msg mono muted">{info.host}:{info.port}</div>

        <div class="keys">
          <div class="krow">
            <span class="lbl">Pinned</span>
            {#if info.pinned.length}
              <div class="vals">
                {#each info.pinned as k}<span class="mono">{k.fingerprint} <em>({k.key_type})</em></span>{/each}
              </div>
            {:else}
              <span class="none">— none —</span>
            {/if}
          </div>
          <div class="krow">
            <span class="lbl">Server</span>
            {#if info.live.length}
              <div class="vals">
                {#each info.live as k}<span class="mono">{k.fingerprint} <em>({k.key_type})</em></span>{/each}
              </div>
            {:else}
              <span class="none">— unreachable —</span>
            {/if}
          </div>
        </div>

        <div class="verdict {VERDICT[info.status].cls}">
          <span class="badge">{VERDICT[info.status].icon}</span>
          <span>{VERDICT[info.status].text}</span>
        </div>
      {/if}

      <div class="actions">
        <button class="ghost" on:click={() => current && load(current)} disabled={loading || busy}>Rescan</button>
        <span class="spacer"></span>
        {#if info && info.status === "Unpinned"}
          <button class="primary" on:click={pin} disabled={busy}>Pin (trust)</button>
        {/if}
        {#if info && info.status === "Changed"}
          <button on:click={forget} disabled={busy}>Forget</button>
          <button class="danger" on:click={replace} disabled={busy}>Replace stale key</button>
        {/if}
        {#if info && (info.status === "Match" || info.status === "Unreachable")}
          <button class="danger" on:click={forget} disabled={busy}>Forget</button>
        {/if}
        <button on:click={close}>Close</button>
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
    width: min(520px, 94vw);
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
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .msg {
    color: var(--fg-dim);
    font-size: 13px;
    line-height: 1.5;
  }
  .mono {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  }
  .muted {
    color: var(--fg-dim);
  }
  .keys {
    display: flex;
    flex-direction: column;
    gap: 10px;
    background: var(--bg-1);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 12px;
  }
  .krow {
    display: flex;
    gap: 12px;
    font-size: 12.5px;
  }
  .lbl {
    flex: none;
    width: 54px;
    color: var(--fg-dim);
    padding-top: 1px;
  }
  .vals {
    display: flex;
    flex-direction: column;
    gap: 4px;
    overflow-wrap: anywhere;
    min-width: 0;
  }
  .vals em {
    color: var(--fg-dim);
    font-style: normal;
  }
  .none {
    color: var(--fg-dim);
  }
  .verdict {
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: 13px;
    padding: 10px 12px;
    border-radius: 10px;
    border: 1px solid var(--border);
    background: var(--bg-1);
  }
  .verdict .badge {
    font-weight: 700;
    width: 20px;
    text-align: center;
    flex: none;
  }
  .verdict.ok .badge {
    color: #3fb950;
  }
  .verdict.bad {
    border-color: var(--danger);
  }
  .verdict.bad .badge,
  .msg.bad {
    color: var(--danger);
  }
  .verdict.warn .badge {
    color: #d29922;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 4px;
  }
  .spacer {
    flex: 1;
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
  button.ghost {
    background: none;
  }
  button:disabled {
    opacity: 0.5;
    pointer-events: none;
  }
</style>
