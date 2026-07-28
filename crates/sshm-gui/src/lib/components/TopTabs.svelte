<script lang="ts">
  import { sessions, activeView, closeSession, closedSessions } from "../stores";
  import { openLocalSession } from "../ipc";
  import Icon from "./Icon.svelte";

  // On macOS the window uses an overlay title bar, so the traffic-light buttons
  // float over the top-left — inset the tabs so they don't collide.
  const isMac =
    typeof navigator !== "undefined" && /Mac/i.test(navigator.platform || navigator.userAgent);
</script>

<div class="tabs" class:mac={isMac} data-tauri-drag-region>
  <button
    class="tab home"
    class:active={$activeView === "manager"}
    on:click={() => activeView.set("manager")}
    title="Host manager"
  >
    <Icon name="hosts" size={15} />
    <span>sshm</span>
  </button>

  {#each $sessions as s (s.id)}
    <div class="tab session" class:active={$activeView === s.id}>
      <button class="label" on:click={() => activeView.set(s.id)}>
        <span class="dot" class:closed={$closedSessions.has(s.backendId)}></span>
        <span>{s.title}</span>
      </button>
      <button class="x" title="Close session" on:click={() => closeSession(s.id)}>
        <Icon name="close" size={12} />
      </button>
    </div>
  {/each}

  <button class="newtab" title="New local terminal" on:click={() => openLocalSession()}>
    <Icon name="plus" size={15} />
  </button>

  <div class="drag" data-tauri-drag-region></div>
</div>

<style>
  .tabs {
    display: flex;
    align-items: stretch;
    gap: 4px;
    height: 40px;
    padding: 6px 8px 0;
    background: var(--bg-0);
    border-bottom: 1px solid var(--border);
  }
  .tabs.mac {
    padding-left: 78px;
  }
  .tab {
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 0 12px;
    border: 1px solid transparent;
    border-bottom: none;
    border-radius: 8px 8px 0 0;
    background: transparent;
    color: var(--fg-dim);
    font-size: 13px;
    max-width: 220px;
  }
  .tab.active {
    background: var(--bg-1);
    border-color: var(--border);
    color: var(--fg);
  }
  .tab.session {
    padding: 0 4px 0 12px;
    gap: 4px;
  }
  .tab .label {
    display: flex;
    align-items: center;
    gap: 7px;
    background: none;
    border: none;
    color: inherit;
    padding: 0;
    max-width: 160px;
  }
  .tab .label span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--ok);
    flex: none;
    box-shadow: 0 0 6px rgba(34, 197, 94, 0.5);
  }
  .dot.closed {
    background: var(--fg-faint);
    box-shadow: none;
  }
  .tab .x {
    background: none;
    border: none;
    color: var(--fg-dim);
    padding: 4px;
    border-radius: 6px;
    display: flex;
  }
  .tab .x:hover {
    background: var(--bg-3);
    color: var(--fg);
  }
  .home span {
    font-weight: 700;
    letter-spacing: 0.3px;
  }
  .newtab {
    align-self: center;
    display: flex;
    background: transparent;
    border: none;
    color: var(--fg-dim);
    padding: 6px;
    border-radius: 7px;
    margin: 0 2px 4px;
  }
  .newtab:hover {
    background: var(--bg-3);
    color: var(--fg);
  }
  .drag {
    flex: 1;
  }
</style>
