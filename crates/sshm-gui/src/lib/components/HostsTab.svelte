<script lang="ts">
  import type { Host } from "../bindings";
  import { commands, tryRun } from "../ipc";
  import { hosts, folders, selectedHostName, openSession, pushToast } from "../stores";
  import { hostIcon } from "../hostIcon";
  import HostForm from "./HostForm.svelte";
  import HostDetail from "./HostDetail.svelte";
  import Icon from "./Icon.svelte";

  let query = "";
  let filtered: Host[] = [];
  let folderFilter: string | null = null;
  let editing: Host | null = null;
  let showForm = false;

  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  function onSearch(): void {
    clearTimeout(searchTimer);
    searchTimer = setTimeout(async () => {
      // Route through the core matcher for the rich `tag:`/`host:` syntax.
      filtered = await commands.listHosts(query.trim() ? query : null);
    }, 120);
  }

  // Keep in sync with the store when it changes (live reload / CRUD).
  $: filtered = localFilter($hosts, query);
  function localFilter(all: Host[], q: string): Host[] {
    if (!q.trim()) return all;
    const n = q.toLowerCase();
    return all.filter((h) => h.name.toLowerCase().includes(n) || h.host.toLowerCase().includes(n));
  }

  $: shown = folderFilter ? filtered.filter((h) => h.folder === folderFilter) : filtered;

  interface Group {
    name: string;
    count: number;
  }
  $: groups = buildGroups($hosts);
  function buildGroups(all: Host[]): Group[] {
    const counts = new Map<string, number>();
    for (const h of all) if (h.folder) counts.set(h.folder, (counts.get(h.folder) ?? 0) + 1);
    return [...counts.entries()].sort((a, b) => a[0].localeCompare(b[0])).map(([name, count]) => ({ name, count }));
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
  function onEdit(e: CustomEvent<Host>): void {
    editing = e.detail;
    showForm = true;
  }
  async function onSaved(): Promise<void> {
    showForm = false;
    await refresh();
  }

  function connectSelectedOrTyped(): void {
    if (selected) {
      openSession(selected.name);
      return;
    }
    if (shown.length === 1) {
      openSession(shown[0].name);
      return;
    }
    pushToast("err", "Pick a host first");
  }

  async function newFolder(): Promise<void> {
    const n = prompt("New folder name:");
    if (!n) return;
    await tryRun(commands.createFolder(n), `Folder "${n}" created`);
    await refresh();
  }
</script>

<div class="wrap">
  <div class="main">
    <div class="searchbar">
      <div class="field">
        <Icon name="search" size={16} />
        <input
          placeholder="Find a host or ssh user@hostname…"
          bind:value={query}
          on:input={onSearch}
          on:keydown={(e) => e.key === "Enter" && connectSelectedOrTyped()}
        />
      </div>
      <button class="primary connect" on:click={connectSelectedOrTyped}>
        <Icon name="connect" size={15} /> CONNECT
      </button>
    </div>

    <div class="toolbar">
      <button class="primary" on:click={newHost}><Icon name="plus" size={14} /> NEW HOST</button>
      <button on:click={newFolder}><Icon name="folder" size={14} /> New folder</button>
      {#if folderFilter}
        <button class="chip" on:click={() => (folderFilter = null)}>
          {folderFilter} <Icon name="close" size={12} />
        </button>
      {/if}
      <div class="spacer"></div>
      <span class="muted small">{shown.length} host{shown.length === 1 ? "" : "s"}</span>
    </div>

    <div class="scroll cards">
      {#if groups.length && !folderFilter && !query.trim()}
        <div class="section">Groups</div>
        <div class="grid">
          {#each groups as g (g.name)}
            <button class="card group" on:click={() => (folderFilter = g.name)}>
              <div class="gic"><Icon name="folder" size={20} /></div>
              <div class="col">
                <div class="ttl">{g.name}</div>
                <div class="sub muted">{g.count} host{g.count === 1 ? "" : "s"}</div>
              </div>
            </button>
          {/each}
        </div>
      {/if}

      <div class="section">Hosts</div>
      <div class="grid">
        {#each shown as h (h.name)}
          <button
            class="card host"
            class:sel={$selectedHostName === h.name}
            on:click={() => selectedHostName.set(h.name)}
            on:dblclick={() => openSession(h.name)}
          >
            <div class="hic" style="background:{hostIcon(h).bg}">{hostIcon(h).label}</div>
            <div class="col grow">
              <div class="ttl">{h.name}</div>
              <div class="sub muted mono">{h.username}@{h.host}</div>
            </div>
            {#if h.tags && h.tags.length}
              <div class="minitags">{#each h.tags.slice(0, 3) as t}<span class="tag">{t}</span>{/each}</div>
            {/if}
          </button>
        {/each}
      </div>
      {#if shown.length === 0}
        <div class="empty muted">No hosts here. Click “NEW HOST” to add one.</div>
      {/if}
    </div>
  </div>

  {#if selected}
    <HostDetail host={selected} on:edit={onEdit} on:changed={refresh} />
  {/if}
</div>

{#if showForm}
  <HostForm host={editing} folders={$folders} on:saved={onSaved} on:cancel={() => (showForm = false)} />
{/if}

<style>
  .wrap {
    display: flex;
    flex: 1;
    min-height: 0;
  }
  .main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    padding: 18px 22px;
    gap: 14px;
  }
  .searchbar {
    display: flex;
    gap: 10px;
  }
  .field {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 8px;
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 0 12px;
    color: var(--fg-dim);
  }
  .field input {
    border: none;
    background: none;
    padding: 11px 0;
  }
  .field:focus-within {
    border-color: var(--accent);
  }
  .connect {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 18px;
    font-weight: 700;
    letter-spacing: 0.4px;
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .toolbar button {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .chip {
    background: var(--accent-soft);
    color: var(--accent);
    border-color: transparent;
  }
  .cards {
    flex: 1;
    padding-right: 4px;
  }
  .section {
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.6px;
    color: var(--fg-dim);
    margin: 14px 2px 8px;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
    gap: 10px;
  }
  .card {
    display: flex;
    align-items: center;
    gap: 12px;
    text-align: left;
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: 12px;
    padding: 12px 14px;
  }
  .card:hover {
    border-color: var(--accent);
    background: var(--bg-3);
  }
  .card.sel {
    border-color: var(--accent);
    background: var(--accent-soft);
  }
  .gic {
    width: 40px;
    height: 40px;
    border-radius: 10px;
    background: linear-gradient(135deg, #1c4fd6, #2b6cff);
    display: grid;
    place-items: center;
    color: #fff;
    flex: none;
  }
  .hic {
    width: 40px;
    height: 40px;
    border-radius: 10px;
    display: grid;
    place-items: center;
    color: #fff;
    font-weight: 700;
    font-size: 17px;
    flex: none;
  }
  .grow {
    flex: 1;
    min-width: 0;
  }
  .ttl {
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sub {
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .minitags {
    display: flex;
    gap: 4px;
    flex: none;
  }
  .small {
    font-size: 12px;
  }
  .empty {
    padding: 40px;
    text-align: center;
  }
</style>
