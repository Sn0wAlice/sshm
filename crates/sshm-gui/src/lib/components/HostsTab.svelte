<script lang="ts">
  import type { Host } from "../bindings";
  import { commands, tryRun } from "../ipc";
  import { hosts, folders, selectedHostName } from "../stores";
  import HostForm from "./HostForm.svelte";

  let query = "";
  let filtered: Host[] = [];
  let editing: Host | null = null;
  let showForm = false;

  // Re-filter through the core matcher (server-side) whenever the query changes.
  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  function onSearch(): void {
    clearTimeout(searchTimer);
    searchTimer = setTimeout(async () => {
      filtered = await commands.listHosts(query.trim() ? query : null);
    }, 120);
  }

  // Keep `filtered` in sync when the store changes (live reload, CRUD).
  $: filtered = applyLocal($hosts, query);
  function applyLocal(all: Host[], q: string): Host[] {
    if (!q.trim()) return all;
    const needle = q.toLowerCase();
    return all.filter(
      (h) =>
        h.name.toLowerCase().includes(needle) ||
        h.host.toLowerCase().includes(needle),
    );
  }

  interface Group {
    name: string;
    hosts: Host[];
  }
  $: grouped = groupByFolder(filtered);
  function groupByFolder(list: Host[]): Group[] {
    const map = new Map<string, Host[]>();
    for (const h of list) {
      const key = h.folder && h.folder.length ? h.folder : "";
      const arr = map.get(key) ?? [];
      arr.push(h);
      map.set(key, arr);
    }
    return [...map.entries()]
      .sort(([a], [b]) => (a === "" ? -1 : b === "" ? 1 : a.localeCompare(b)))
      .map(([name, hs]) => ({ name, hosts: hs }));
  }

  $: selected = $hosts.find((h) => h.name === $selectedHostName) ?? null;

  async function refresh(): Promise<void> {
    hosts.set(await commands.listHosts(null));
    folders.set(await commands.listFolders());
  }

  function newHost(): void {
    editing = null;
    showForm = true;
  }
  function editHost(h: Host): void {
    editing = h;
    showForm = true;
  }
  async function onSaved(): Promise<void> {
    showForm = false;
    await refresh();
  }

  async function connect(h: Host): Promise<void> {
    await tryRun(commands.connectHost(h.name), `Opening ${h.name}…`);
  }
  async function del(h: Host): Promise<void> {
    if (!confirm(`Delete host "${h.name}"?`)) return;
    await tryRun(commands.deleteHost(h.name), `Deleted ${h.name}`);
    if ($selectedHostName === h.name) selectedHostName.set(null);
    await refresh();
  }
  async function clone(h: Host): Promise<void> {
    const nn = prompt(`Clone "${h.name}" as:`, `${h.name}-copy`);
    if (!nn) return;
    await tryRun(commands.cloneHost(h.name, nn), `Cloned to ${nn}`);
    await refresh();
  }

  async function newFolder(): Promise<void> {
    const n = prompt("New folder name:");
    if (!n) return;
    await tryRun(commands.createFolder(n), `Folder "${n}" created`);
    await refresh();
  }
</script>

<div class="hosts">
  <div class="list col">
    <div class="row toolbar">
      <input
        placeholder="Search hosts (tag:foo host:1.* user:bar)…"
        bind:value={query}
        on:input={onSearch}
      />
      <button class="primary" on:click={newHost}>+ Host</button>
      <button on:click={newFolder} title="New folder">📁+</button>
    </div>

    <div class="scroll groups">
      {#each grouped as g}
        {#if g.name}<div class="folder">{g.name}</div>{/if}
        {#each g.hosts as h (h.name)}
          <button
            class="item"
            class:sel={$selectedHostName === h.name}
            on:click={() => selectedHostName.set(h.name)}
            on:dblclick={() => connect(h)}
          >
            <span class="dot" class:fav={h.favorite}></span>
            <span class="nm">{h.name}</span>
            <span class="addr muted mono">{h.username}@{h.host}</span>
          </button>
        {/each}
      {/each}
      {#if filtered.length === 0}
        <div class="empty muted">No hosts. Click “+ Host” to add one.</div>
      {/if}
    </div>
  </div>

  <div class="detail col">
    {#if selected}
      <div class="row">
        <h2>{selected.name}</h2>
        {#if selected.favorite}<span class="tag">★ favorite</span>{/if}
        <div class="spacer"></div>
        <button class="primary" on:click={() => selected && connect(selected)}>Connect ▸</button>
      </div>
      <div class="kv mono">
        <div><span class="muted">host</span> {selected.username}@{selected.host}:{selected.port}</div>
        {#if selected.identity_file}<div><span class="muted">identity</span> {selected.identity_file}</div>{/if}
        {#if selected.proxy_jump}<div><span class="muted">proxyjump</span> {selected.proxy_jump}</div>{/if}
        {#if selected.folder}<div><span class="muted">folder</span> {selected.folder}</div>{/if}
        <div><span class="muted">connections</span> {selected.use_count}</div>
        {#if selected.forward_agent}<div><span class="tag">-A forward agent</span></div>{/if}
        {#if selected.mosh}<div><span class="tag">mosh</span></div>{/if}
      </div>
      {#if selected.tags && selected.tags.length}
        <div class="row">{#each selected.tags as t}<span class="tag">{t}</span>{/each}</div>
      {/if}
      {#if selected.remote_command}
        <div class="mono muted">run: {selected.remote_command}</div>
      {/if}
      {#if selected.notes}
        <div class="notes">{selected.notes}</div>
      {/if}
      {#if (selected.tunnels ?? []).length}
        <div class="muted">Saved tunnels: {(selected.tunnels ?? []).length} (manage in Tunnels tab)</div>
      {/if}

      <div class="row actions">
        <button on:click={() => selected && editHost(selected)}>Edit</button>
        <button on:click={() => selected && clone(selected)}>Clone</button>
        <button class="danger" on:click={() => selected && del(selected)}>Delete</button>
      </div>
    {:else}
      <div class="placeholder muted">Select a host to see details.</div>
    {/if}
  </div>
</div>

{#if showForm}
  <HostForm
    host={editing}
    folders={$folders}
    on:saved={onSaved}
    on:cancel={() => (showForm = false)}
  />
{/if}

<style>
  .hosts {
    display: grid;
    grid-template-columns: 340px 1fr;
    height: 100%;
    min-height: 0;
  }
  .list {
    border-right: 1px solid var(--border);
    min-height: 0;
  }
  .toolbar {
    padding: 10px;
    border-bottom: 1px solid var(--border);
  }
  .toolbar button {
    white-space: nowrap;
  }
  .groups {
    flex: 1;
    padding: 6px;
  }
  .folder {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--fg-dim);
    padding: 10px 8px 4px;
  }
  .item {
    width: 100%;
    text-align: left;
    background: transparent;
    border: none;
    border-radius: var(--radius);
    padding: 7px 8px;
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .item:hover {
    background: var(--bg-2);
  }
  .item.sel {
    background: var(--bg-3);
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--border);
    flex: none;
  }
  .dot.fav {
    background: var(--accent-2);
  }
  .nm {
    font-weight: 600;
  }
  .addr {
    margin-left: auto;
    font-size: 12px;
  }
  .empty {
    padding: 24px;
    text-align: center;
  }
  .detail {
    padding: 18px;
    overflow-y: auto;
    gap: 12px;
  }
  h2 {
    margin: 0;
  }
  .kv {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 13px;
  }
  .notes {
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 10px;
    white-space: pre-wrap;
  }
  .actions {
    margin-top: auto;
    padding-top: 12px;
  }
  .placeholder {
    margin: auto;
  }
</style>
