<script lang="ts">
  import { onMount } from "svelte";
  import { fly } from "svelte/transition";
  import { flip } from "svelte/animate";
  import type { Host } from "../bindings";
  import { commands, tryRun, openHostSession } from "../ipc";
  import { hosts, folders, selectedHostName, pushToast, newHostRequest } from "../stores";
  import { confirmDialog, promptDialog } from "../dialogs";
  import { hostIcon } from "../hostIcon";
  import HostForm from "./HostForm.svelte";
  import HostDetail from "./HostDetail.svelte";
  import Icon from "./Icon.svelte";

  let query = "";
  let filtered: Host[] = [];
  let currentPath = ""; // "" = root; folders drill down, non-recursive
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

  // --- Folder navigation (drill-down, not a flat recursive list) ---
  $: searching = query.trim().length > 0;

  // Every folder path that exists (each nested path expanded to its ancestors).
  $: allPaths = collectFolderPaths($folders, $hosts);
  function collectFolderPaths(fs: string[], all: Host[]): Set<string> {
    const set = new Set<string>();
    const add = (f: string) => {
      let acc = "";
      for (const seg of f.split("/").filter(Boolean)) {
        acc = acc ? `${acc}/${seg}` : seg;
        set.add(acc);
      }
    };
    for (const f of fs) add(f);
    for (const h of all) if (h.folder) add(h.folder);
    return set;
  }

  // Immediate children of the current folder only.
  $: subfolders = childFolders(allPaths, currentPath);
  function childFolders(paths: Set<string>, path: string): string[] {
    const out: string[] = [];
    for (const p of paths) {
      const idx = p.lastIndexOf("/");
      const parent = idx === -1 ? "" : p.slice(0, idx);
      if (parent === path) out.push(p);
    }
    return out.sort((a, b) => a.localeCompare(b));
  }

  // Hosts to show: when searching, the flat match set; otherwise just the hosts
  // that live directly in the current folder (root = no folder).
  $: dirHosts = searching ? filtered : filtered.filter((h) => (h.folder ?? "") === currentPath);

  function subtreeCount(path: string): number {
    return $hosts.filter((h) => h.folder === path || (h.folder ?? "").startsWith(`${path}/`)).length;
  }
  function folderLabel(path: string): string {
    return path.split("/").pop() ?? path;
  }
  function openFolder(path: string): void {
    currentPath = path;
    selectedHostName.set(null);
  }
  function goUp(): void {
    const idx = currentPath.lastIndexOf("/");
    currentPath = idx === -1 ? "" : currentPath.slice(0, idx);
  }

  $: selected = $hosts.find((h) => h.name === $selectedHostName) ?? null;

  // --- Reachability (presence dots + latency) ---
  let pings: Record<string, number | null> = {};
  async function pollPings(): Promise<void> {
    const r = await commands.pingHosts();
    const next: Record<string, number | null> = {};
    for (const p of r) next[p.name] = p.latency_ms;
    pings = next;
  }
  onMount(() => {
    pollPings();
    const t = setInterval(pollPings, 15000);
    return () => clearInterval(t);
  });
  function presence(name: string): "up" | "unknown" {
    return typeof pings[name] === "number" ? "up" : "unknown";
  }

  async function refresh(): Promise<void> {
    hosts.set(await commands.listHosts(null));
    folders.set(await commands.listFolders());
  }

  function newHost(): void {
    editing = null;
    showForm = true;
  }
  // The command palette can request the New-host form.
  let lastNewHostReq = 0;
  $: if ($newHostRequest !== lastNewHostReq) {
    lastNewHostReq = $newHostRequest;
    newHost();
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
    if (dirHosts.length === 1) {
      openHostSession(dirHosts[0].name);
    } else if (dirHosts.length === 0) {
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
    openHostSession(h.name);
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
    const nn = await promptDialog({
      title: `Duplicate "${h.name}"`,
      placeholder: "New host name",
      initial: `${h.name}-copy`,
      confirmLabel: "Duplicate",
    });
    if (!nn) return;
    await tryRun(commands.cloneHost(h.name, nn), `Duplicated to ${nn}`);
    await refresh();
  }
  async function mDelete(h: Host): Promise<void> {
    closeMenu();
    const ok = await confirmDialog({
      title: `Delete "${h.name}"?`,
      message: "This removes the host from your vault.",
      confirmLabel: "Delete",
      danger: true,
    });
    if (!ok) return;
    await tryRun(commands.deleteHost(h.name));
    if ($selectedHostName === h.name) selectedHostName.set(null);
    await refresh();
    // Undo re-saves the exact host that was removed.
    pushToast("info", `Deleted ${h.name}`, {
      label: "Undo",
      run: async () => {
        await tryRun(commands.saveHost(h, null));
        await refresh();
      },
    });
  }

  async function newFolder(): Promise<void> {
    const n = await promptDialog({ title: "New folder", placeholder: "Folder name", confirmLabel: "Create" });
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
      <button class="primary" on:click={newHost}><Icon name="plus" size={14} /> New host</button>
      <button on:click={newFolder}><Icon name="folder" size={14} /> New folder</button>
      {#if !searching && currentPath}
        <div class="crumbs">
          <button class="crumb" on:click={() => openFolder("")}>Home</button>
          {#each currentPath.split("/") as seg, i}
            <span class="sep">/</span>
            <button class="crumb" on:click={() => openFolder(currentPath.split("/").slice(0, i + 1).join("/"))}>{seg}</button>
          {/each}
        </div>
      {/if}
      <div class="spacer"></div>
      <span class="muted small">{dirHosts.length} host{dirHosts.length === 1 ? "" : "s"}</span>
    </div>

    <div class="scroll cards">
      {#if !searching && (currentPath || subfolders.length)}
        <div class="section">Folders</div>
        <div class="grid">
          {#if currentPath}
            <button class="card folder up" on:click={goUp}>
              <div class="gic"><Icon name="folder" size={18} /></div>
              <div class="col"><div class="ttl">..</div><div class="sub muted">back</div></div>
            </button>
          {/if}
          {#each subfolders as f (f)}
            <button class="card folder" on:click={() => openFolder(f)}>
              <div class="gic"><Icon name="folder" size={18} /></div>
              <div class="col">
                <div class="ttl">{folderLabel(f)}</div>
                <div class="sub muted">{subtreeCount(f)} host{subtreeCount(f) === 1 ? "" : "s"}</div>
              </div>
            </button>
          {/each}
        </div>
      {/if}

      {#if dirHosts.length}
        <div class="section">{searching ? "Results" : "Hosts"}</div>
        <div class="grid">
          {#each dirHosts as h (h.name)}
            <button
              class="card host"
              class:sel={$selectedHostName === h.name}
              title="Click to connect · right-click for options"
              animate:flip={{ duration: 160 }}
              in:fly={{ y: 6, duration: 140 }}
              on:click={() => openHostSession(h.name)}
              on:contextmenu={(e) => openMenu(e, h)}
            >
              <div class="hic-wrap">
                <div class="hic" style="background:{hostIcon(h).bg}">{hostIcon(h).label}</div>
                <span class="pres {presence(h.name)}" title={presence(h.name) === "up" ? "reachable" : "unknown"}></span>
              </div>
              <div class="col grow">
                <div class="ttl">{h.name}</div>
                <div class="sub muted mono">
                  {h.username}@{h.host}{#if typeof pings[h.name] === "number"} · {pings[h.name]}ms{/if}
                </div>
              </div>
              {#if h.tags && h.tags.length}
                <div class="minitags">{#each h.tags.slice(0, 3) as t}<span class="tag">{t}</span>{/each}</div>
              {/if}
            </button>
          {/each}
        </div>
      {:else if !subfolders.length}
        <div class="empty">
          {#if searching}
            <div class="muted">No matching host.</div>
          {:else}
            <div class="e-ico"><Icon name="hosts" size={26} /></div>
            <div class="e-title">Nothing here yet</div>
            <div class="muted">Add a host to get started.</div>
            <button class="primary" on:click={newHost}><Icon name="plus" size={14} /> New host</button>
          {/if}
        </div>
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
  .crumbs {
    display: flex;
    align-items: center;
    gap: 2px;
    font-size: 12.5px;
  }
  .crumbs .crumb {
    background: none;
    border: none;
    padding: 3px 6px;
    color: var(--fg-dim);
    border-radius: 6px;
  }
  .crumbs .crumb:hover {
    background: var(--bg-3);
    color: var(--fg);
  }
  .crumbs .sep {
    color: var(--fg-faint);
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
    border-radius: 11px;
    padding: 11px 13px;
  }
  .card:hover {
    border-color: #3a3a40;
    background: var(--bg-3);
  }
  .card.sel {
    border-color: var(--ring);
    background: var(--bg-3);
    box-shadow: 0 0 0 1px var(--ring) inset;
  }
  .card.folder .ttl {
    font-weight: 600;
  }
  .card.up .gic {
    opacity: 0.7;
  }
  .gic {
    width: 38px;
    height: 38px;
    border-radius: 9px;
    background: var(--bg-4);
    border: 1px solid var(--border);
    display: grid;
    place-items: center;
    color: var(--fg-dim);
    flex: none;
  }
  .hic-wrap {
    position: relative;
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
  }
  .pres {
    position: absolute;
    right: -3px;
    bottom: -3px;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    border: 2.5px solid var(--bg-2);
    background: var(--fg-faint);
  }
  .card:hover .pres {
    border-color: var(--bg-3);
  }
  .pres.up {
    background: var(--ok);
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
    padding: 48px 40px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
  }
  .e-ico {
    width: 52px;
    height: 52px;
    border-radius: 14px;
    background: var(--bg-2);
    border: 1px solid var(--border);
    display: grid;
    place-items: center;
    color: var(--fg-dim);
    margin-bottom: 6px;
  }
  .e-title {
    font-weight: 600;
    font-size: 15px;
  }
  .empty .primary {
    margin-top: 8px;
    display: flex;
    align-items: center;
    gap: 6px;
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
