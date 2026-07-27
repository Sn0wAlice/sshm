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
  let menu: { host: Host; x: number; y: number } | null = null;

  // Single source of truth: the core matcher (case-insensitive, `tag:`/`host:`
  // syntax). Re-run whenever the query OR the host list changes (live reload /
  // CRUD); debounced only for the async server hop while typing.
  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  $: search($hosts, query);
  function search(all: Host[], q: string): void {
    clearTimeout(searchTimer);
    if (!q.trim()) {
      filtered = all;
      return;
    }
    searchTimer = setTimeout(async () => {
      filtered = await commands.listHosts(q);
    }, 90);
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

  // Enter / CONNECT: connect when the search narrows to a single host.
  function connectFromSearch(): void {
    if (shown.length === 1) {
      openSession(shown[0].name);
    } else if (shown.length === 0) {
      pushToast("err", "No matching host");
    } else {
      pushToast("err", "Narrow the search to one host, or click a host to connect");
    }
  }

  // ----- Right-click context menu -----
  function openMenu(e: MouseEvent, h: Host): void {
    e.preventDefault();
    menu = {
      host: h,
      x: Math.min(e.clientX, window.innerWidth - 220),
      y: Math.min(e.clientY, window.innerHeight - 280),
    };
  }
  function closeMenu(): void {
    menu = null;
  }
  function mConnect(h: Host): void {
    closeMenu();
    openSession(h.name);
  }
  async function mExternal(h: Host): Promise<void> {
    closeMenu();
    await tryRun(commands.connectHost(h.name), `Opening ${h.name} in your terminal…`);
  }
  function mDetails(h: Host): void {
    closeMenu();
    selectedHostName.set(h.name);
  }
  function mEdit(h: Host): void {
    closeMenu();
    editing = h;
    showForm = true;
  }
  async function mClone(h: Host): Promise<void> {
    closeMenu();
    const nn = prompt(`Duplicate "${h.name}" as:`, `${h.name}-copy`);
    if (!nn) return;
    await tryRun(commands.cloneHost(h.name, nn), `Duplicated to ${nn}`);
    await refresh();
  }
  async function mDelete(h: Host): Promise<void> {
    closeMenu();
    if (!confirm(`Delete host "${h.name}"?`)) return;
    await tryRun(commands.deleteHost(h.name), `Deleted ${h.name}`);
    if ($selectedHostName === h.name) selectedHostName.set(null);
    await refresh();
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
          placeholder="Find a host (case-insensitive · tag:prod host:10.* user:bar)…"
          bind:value={query}
          on:keydown={(e) => e.key === "Enter" && connectFromSearch()}
        />
      </div>
      <button class="primary connect" on:click={connectFromSearch}>
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
            title="Click to connect · right-click for options"
            on:click={() => openSession(h.name)}
            on:contextmenu={(e) => openMenu(e, h)}
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

{#if menu}
  {@const m = menu}
  <div class="ctxmenu" style="left:{m.x}px; top:{m.y}px" role="menu">
    <button on:click={() => mConnect(m.host)}>Connect</button>
    <button on:click={() => mExternal(m.host)}>Open in external terminal</button>
    <div class="sep"></div>
    <button on:click={() => mDetails(m.host)}>Details</button>
    <button on:click={() => mEdit(m.host)}>Edit settings…</button>
    <button on:click={() => mClone(m.host)}>Duplicate</button>
    <div class="sep"></div>
    <button class="del" on:click={() => mDelete(m.host)}>Delete</button>
  </div>
{/if}

<svelte:window
  on:click={closeMenu}
  on:keydown={(e) => e.key === "Escape" && closeMenu()}
/>

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
  .ctxmenu {
    position: fixed;
    z-index: 60;
    min-width: 210px;
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 6px;
    display: flex;
    flex-direction: column;
    box-shadow: 0 14px 40px rgba(0, 0, 0, 0.5);
  }
  .ctxmenu button {
    background: none;
    border: none;
    text-align: left;
    padding: 8px 10px;
    border-radius: 7px;
    font-size: 13px;
  }
  .ctxmenu button:hover {
    background: var(--bg-3);
  }
  .ctxmenu button.del {
    color: var(--danger);
  }
  .ctxmenu button.del:hover {
    background: var(--danger-soft);
  }
  .ctxmenu .sep {
    height: 1px;
    background: var(--border);
    margin: 5px 6px;
  }
</style>
