<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { SearchAddon } from "@xterm/addon-search";
  import { WebLinksAddon } from "@xterm/addon-web-links";
  import "@xterm/xterm/css/xterm.css";
  import { commands, events } from "../ipc";

  /** The already-spawned backend PTY session id this terminal is attached to. */
  export let backendId: string;

  let el: HTMLDivElement;
  let term: Terminal;
  let fit: FitAddon;
  let search: SearchAddon;
  let alive = true;
  const cleanups: Array<() => void> = [];

  let searchOpen = false;
  let searchTerm = "";
  let searchEl: HTMLInputElement | undefined;

  function openSearch(): void {
    searchOpen = true;
    requestAnimationFrame(() => searchEl?.focus());
  }
  function closeSearch(): void {
    searchOpen = false;
    search?.clearDecorations();
    term?.focus();
  }
  function findNext(): void {
    if (searchTerm) search?.findNext(searchTerm);
  }
  function findPrev(): void {
    if (searchTerm) search?.findPrevious(searchTerm);
  }
  // ⌘F / Ctrl+F opens the in-terminal search (captured before xterm).
  function onWrapKey(e: KeyboardEvent): void {
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "f") {
      e.preventDefault();
      e.stopPropagation();
      openSearch();
    }
  }

  onMount(async () => {
    term = new Terminal({
      fontFamily:
        'ui-monospace, "SF Mono", Menlo, "Cascadia Code", Consolas, monospace',
      fontSize: 13,
      cursorBlink: true,
      allowProposedApi: true,
      theme: {
        background: "#09090b",
        foreground: "#e4e4e7",
        cursor: "#fafafa",
        cursorAccent: "#09090b",
        selectionBackground: "#3f3f46",
        black: "#18181b",
        brightBlack: "#52525b",
      },
    });
    fit = new FitAddon();
    search = new SearchAddon();
    term.loadAddon(fit);
    term.loadAddon(search);
    term.loadAddon(new WebLinksAddon());
    term.open(el);
    fit.fit();
    void commands.termResize(backendId, term.cols, term.rows);

    // Input → PTY.
    const onData = term.onData((d) => {
      void commands.termWrite(backendId, Array.from(new TextEncoder().encode(d)));
    });
    cleanups.push(() => onData.dispose());

    // PTY output → terminal (filtered by our session id).
    const unOut = await events.termOutputEvent.listen((e) => {
      if (e.payload.id === backendId) term.write(new Uint8Array(e.payload.data));
    });
    const unExit = await events.termExitEvent.listen((e) => {
      if (e.payload.id === backendId) term.writeln("\r\n\x1b[90m[session closed]\x1b[0m");
    });
    cleanups.push(unOut, unExit);

    // Keep the PTY size in sync with the widget.
    const ro = new ResizeObserver(() => {
      if (!alive) return;
      try {
        fit.fit();
        void commands.termResize(backendId, term.cols, term.rows);
      } catch {
        /* element not measurable yet */
      }
    });
    ro.observe(el);
    cleanups.push(() => ro.disconnect());

    term.focus();
  });

  onDestroy(() => {
    alive = false;
    for (const c of cleanups) c();
    void commands.termClose(backendId);
    term?.dispose();
  });
</script>

<!-- The gap lives on the wrapper's padding; xterm goes in the inner box that
     the FitAddon measures, so the empty space below is real (not "filled" by
     extra rows). -->
<div class="wrap" on:keydown|capture={onWrapKey} role="presentation">
  {#if searchOpen}
    <div class="search">
      <input
        bind:this={searchEl}
        bind:value={searchTerm}
        placeholder="Search…"
        spellcheck="false"
        on:input={findNext}
        on:keydown|stopPropagation={(e) => {
          if (e.key === "Enter") (e.shiftKey ? findPrev() : findNext());
          else if (e.key === "Escape") closeSearch();
        }}
      />
      <button on:click={findPrev} title="Previous">↑</button>
      <button on:click={findNext} title="Next">↓</button>
      <button on:click={closeSearch} title="Close">✕</button>
    </div>
  {/if}
  <div class="term" bind:this={el}></div>
</div>

<style>
  .wrap {
    position: relative;
    width: 100%;
    height: 100%;
    background: #09090b;
    padding: 8px 12px 22px;
    display: flex;
    flex-direction: column;
  }
  .search {
    position: absolute;
    top: 8px;
    right: 16px;
    z-index: 5;
    display: flex;
    gap: 4px;
    align-items: center;
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: 9px;
    padding: 5px 6px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.5);
  }
  .search input {
    width: 180px;
    padding: 5px 8px;
    border-radius: 6px;
  }
  .search button {
    padding: 4px 8px;
    border-radius: 6px;
    background: transparent;
    border: none;
    color: var(--fg-dim);
  }
  .search button:hover {
    background: var(--bg-3);
    color: var(--fg);
  }
  .term {
    flex: 1;
    min-height: 0;
    width: 100%;
  }
  :global(.xterm) {
    height: 100%;
  }
  :global(.xterm-viewport) {
    background-color: transparent !important;
  }
  :global(.xterm-viewport)::-webkit-scrollbar {
    width: 8px;
  }
</style>
