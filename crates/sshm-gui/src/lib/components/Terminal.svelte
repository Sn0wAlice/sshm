<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import "@xterm/xterm/css/xterm.css";
  import { commands, events } from "../ipc";

  /** The already-spawned backend PTY session id this terminal is attached to. */
  export let backendId: string;

  let el: HTMLDivElement;
  let term: Terminal;
  let fit: FitAddon;
  let alive = true;
  const cleanups: Array<() => void> = [];

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
    term.loadAddon(fit);
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
<div class="wrap">
  <div class="term" bind:this={el}></div>
</div>

<style>
  .wrap {
    width: 100%;
    height: 100%;
    background: #09090b;
    padding: 8px 12px 22px;
    display: flex;
    flex-direction: column;
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
