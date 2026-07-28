<script lang="ts">
  import { fade, scale } from "svelte/transition";
  import { Server, SquareTerminal, Plus, Waypoints, Boxes, KeyRound, Settings } from "lucide-svelte";
  import { paletteOpen, hosts, activeSection, newHostRequest, type Section } from "../stores";
  import { openHostSession, openLocalSession } from "../ipc";

  interface Item {
    id: string;
    label: string;
    sub?: string;
    group: string;
    icon: any;
    run: () => void;
  }

  let query = "";
  let index = 0;
  let inputEl: HTMLInputElement | undefined;

  function go(s: Section): void {
    activeSection.set(s);
  }

  $: staticItems = [
    { id: "act-new-host", label: "New host", group: "Actions", icon: Plus, run: () => { activeSection.set("hosts"); newHostRequest.update((n) => n + 1); } },
    { id: "act-local", label: "New local terminal", group: "Actions", icon: SquareTerminal, run: () => openLocalSession() },
    { id: "nav-hosts", label: "Go to Hosts", group: "Navigate", icon: Server, run: () => go("hosts") },
    { id: "nav-pf", label: "Go to Port forwarding", group: "Navigate", icon: Waypoints, run: () => go("portforward") },
    { id: "nav-kl", label: "Go to Kluster", group: "Navigate", icon: Boxes, run: () => go("kluster") },
    { id: "nav-kc", label: "Go to Keychain", group: "Navigate", icon: KeyRound, run: () => go("keychain") },
    { id: "nav-set", label: "Go to Settings", group: "Navigate", icon: Settings, run: () => go("settings") },
  ] as Item[];

  $: hostItems = $hosts.map<Item>((h) => ({
    id: `host-${h.name}`,
    label: h.name,
    sub: `${h.username}@${h.host}`,
    group: "Connect",
    icon: Server,
    run: () => openHostSession(h.name),
  }));

  $: all = [...hostItems, ...staticItems];

  $: results = filter(all, query);
  function filter(items: Item[], q: string): Item[] {
    const n = q.trim().toLowerCase();
    if (!n) return items;
    return items
      .map((it) => ({ it, score: score(`${it.label} ${it.sub ?? ""}`.toLowerCase(), n) }))
      .filter((x) => x.score >= 0)
      .sort((a, b) => a.score - b.score)
      .map((x) => x.it);
  }
  // Small subsequence score: lower is better; -1 = no match.
  function score(hay: string, needle: string): number {
    if (hay.includes(needle)) return hay.indexOf(needle);
    let hi = 0;
    for (const ch of needle) {
      hi = hay.indexOf(ch, hi);
      if (hi === -1) return -1;
      hi++;
    }
    return 500; // fuzzy subsequence match, ranked after substring hits
  }

  $: if (index >= results.length) index = Math.max(0, results.length - 1);

  // Reset + focus each time it opens.
  let wasOpen = false;
  $: if ($paletteOpen && !wasOpen) {
    wasOpen = true;
    query = "";
    index = 0;
    requestAnimationFrame(() => inputEl?.focus());
  } else if (!$paletteOpen) {
    wasOpen = false;
  }

  function exec(it: Item | undefined): void {
    if (!it) return;
    paletteOpen.set(false);
    it.run();
  }
  function onKey(e: KeyboardEvent): void {
    if (e.key === "Escape") {
      paletteOpen.set(false);
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      index = Math.min(index + 1, results.length - 1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      index = Math.max(index - 1, 0);
    } else if (e.key === "Enter") {
      e.preventDefault();
      exec(results[index]);
    }
  }
</script>

{#if $paletteOpen}
  <div class="backdrop" transition:fade={{ duration: 100 }} on:click|self={() => paletteOpen.set(false)} role="presentation">
    <div class="palette" transition:scale={{ duration: 130, start: 0.98 }}>
      <input
        bind:this={inputEl}
        bind:value={query}
        on:keydown={onKey}
        placeholder="Search hosts, run a command…"
        spellcheck="false"
      />
      <div class="list">
        {#each results as it, i (it.id)}
          <button
            class="item"
            class:active={i === index}
            on:mousemove={() => (index = i)}
            on:click={() => exec(it)}
          >
            <svelte:component this={it.icon} size={16} class="ic" />
            <span class="lbl">{it.label}</span>
            {#if it.sub}<span class="sub mono">{it.sub}</span>{/if}
            <span class="grp">{it.group}</span>
          </button>
        {:else}
          <div class="empty muted">No results</div>
        {/each}
      </div>
      <div class="hint muted">
        <span><kbd>↑</kbd><kbd>↓</kbd> navigate</span>
        <span><kbd>↵</kbd> run</span>
        <span><kbd>esc</kbd> close</span>
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    backdrop-filter: blur(2px);
    display: flex;
    justify-content: center;
    align-items: flex-start;
    padding-top: 14vh;
    z-index: 85;
  }
  .palette {
    width: min(600px, 92vw);
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: 14px;
    box-shadow: 0 30px 80px rgba(0, 0, 0, 0.65);
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }
  .palette > input {
    border: none;
    border-bottom: 1px solid var(--border);
    border-radius: 0;
    background: transparent;
    padding: 15px 16px;
    font-size: 15px;
  }
  .palette > input:focus {
    box-shadow: none;
  }
  .list {
    max-height: 46vh;
    overflow-y: auto;
    padding: 6px;
  }
  .item {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 10px;
    background: transparent;
    border: none;
    text-align: left;
    padding: 9px 10px;
    border-radius: 8px;
  }
  .item.active {
    background: var(--bg-3);
  }
  .item :global(.ic) {
    color: var(--fg-dim);
    flex: none;
  }
  .lbl {
    font-weight: 500;
  }
  .sub {
    color: var(--fg-dim);
    font-size: 12px;
  }
  .grp {
    margin-left: auto;
    font-size: 11px;
    color: var(--fg-faint);
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }
  .empty {
    padding: 24px;
    text-align: center;
  }
  .hint {
    display: flex;
    gap: 16px;
    padding: 8px 14px;
    border-top: 1px solid var(--border);
    font-size: 11px;
  }
  kbd {
    background: var(--bg-3);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0 5px;
    margin-right: 2px;
    font-family: inherit;
  }
</style>
