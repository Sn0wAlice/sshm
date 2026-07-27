<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import type { Host } from "../bindings";
  import { commands, tryRun, openHostSession } from "../ipc";
  import { selectedHostName } from "../stores";
  import { hostIcon } from "../hostIcon";
  import Icon from "./Icon.svelte";

  export let host: Host;

  const dispatch = createEventDispatcher<{ edit: Host; changed: void }>();

  $: ic = hostIcon(host);

  function connect(): void {
    openHostSession(host.name);
  }
  async function external(): Promise<void> {
    await tryRun(commands.connectHost(host.name), `Opening ${host.name} in your terminal…`);
  }
  async function clone(): Promise<void> {
    const nn = prompt(`Clone "${host.name}" as:`, `${host.name}-copy`);
    if (!nn) return;
    await tryRun(commands.cloneHost(host.name, nn), `Cloned to ${nn}`);
    dispatch("changed");
  }
  async function del(): Promise<void> {
    if (!confirm(`Delete host "${host.name}"?`)) return;
    await tryRun(commands.deleteHost(host.name), `Deleted ${host.name}`);
    selectedHostName.set(null);
    dispatch("changed");
  }
</script>

<aside class="drawer">
  <div class="head">
    <div class="ic" style="background:{ic.bg}">{ic.label}</div>
    <div class="col grow">
      <div class="name">{host.name}</div>
      <div class="mono muted">{host.username}@{host.host}:{host.port}</div>
    </div>
    <button class="ghost" on:click={() => selectedHostName.set(null)} title="Close">
      <Icon name="close" size={16} />
    </button>
  </div>

  <div class="actions">
    <button class="primary" on:click={connect}>
      <Icon name="terminal" size={15} /> Connect
    </button>
    <button on:click={external} title="Open in your external terminal">External ↗</button>
  </div>

  <div class="kv">
    {#if host.identity_file}<div><span class="k">Identity</span><span class="mono">{host.identity_file}</span></div>{/if}
    {#if host.proxy_jump}<div><span class="k">ProxyJump</span><span class="mono">{host.proxy_jump}</span></div>{/if}
    {#if host.folder}<div><span class="k">Folder</span><span>{host.folder}</span></div>{/if}
    <div><span class="k">Connections</span><span>{host.use_count ?? 0}</span></div>
    {#if host.forward_agent}<div><span class="k">Agent</span><span>forwarded (-A)</span></div>{/if}
    {#if host.mosh}<div><span class="k">Transport</span><span>mosh</span></div>{/if}
    {#if host.remote_command}<div><span class="k">On connect</span><span class="mono">{host.remote_command}</span></div>{/if}
  </div>

  {#if host.tags && host.tags.length}
    <div class="tags">{#each host.tags as t}<span class="tag">{t}</span>{/each}</div>
  {/if}
  {#if host.notes}
    <div class="notes">{host.notes}</div>
  {/if}
  {#if (host.tunnels ?? []).length}
    <div class="muted small">{(host.tunnels ?? []).length} saved tunnel(s) — start them in Port forwarding.</div>
  {/if}

  <div class="foot">
    <button on:click={() => dispatch("edit", host)}>Edit</button>
    <button on:click={clone}>Clone</button>
    <button class="danger" on:click={del}>Delete</button>
  </div>
</aside>

<style>
  .drawer {
    width: 340px;
    border-left: 1px solid var(--border);
    background: var(--bg-0);
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding: 18px;
    overflow-y: auto;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .ic {
    width: 44px;
    height: 44px;
    border-radius: 12px;
    display: grid;
    place-items: center;
    font-weight: 700;
    color: #fff;
    font-size: 18px;
    flex: none;
  }
  .grow {
    flex: 1;
    min-width: 0;
  }
  .name {
    font-weight: 700;
    font-size: 16px;
  }
  .actions {
    display: flex;
    gap: 8px;
  }
  .actions .primary {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
  }
  .kv {
    display: flex;
    flex-direction: column;
    gap: 8px;
    font-size: 13px;
  }
  .kv > div {
    display: flex;
    justify-content: space-between;
    gap: 12px;
  }
  .kv .k {
    color: var(--fg-dim);
  }
  .kv span:last-child {
    text-align: right;
    overflow-wrap: anywhere;
  }
  .tags {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .notes {
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 10px;
    white-space: pre-wrap;
    font-size: 13px;
  }
  .small {
    font-size: 12px;
  }
  .foot {
    margin-top: auto;
    display: flex;
    gap: 8px;
  }
  .ghost {
    background: none;
    border: none;
    color: var(--fg-dim);
    padding: 4px;
  }
</style>
