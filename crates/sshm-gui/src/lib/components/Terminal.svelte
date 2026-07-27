<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import "@xterm/xterm/css/xterm.css";
  import { commands, events } from "../ipc";
  import { pushToast } from "../stores";

  export let host: string;

  let el: HTMLDivElement;
  let term: Terminal;
  let fit: FitAddon;
  let backendId: string | null = null;
  let disposed = false;
  const cleanups: Array<() => void> = [];

  onMount(async () => {
    term = new Terminal({
      fontFamily:
        'ui-monospace, "SF Mono", Menlo, "Cascadia Code", Consolas, monospace',
      fontSize: 13,
      cursorBlink: true,
      allowProposedApi: true,
      theme: {
        background: "#0b0f18",
        foreground: "#d6deeb",
        cursor: "#5aa7ff",
        selectionBackground: "#25406b",
        black: "#0b0f18",
        brightBlack: "#5b6b83",
      },
    });
    fit = new FitAddon();
    term.loadAddon(fit);
    term.open(el);
    fit.fit();

    const r = await commands.termOpen(host, term.cols, term.rows);
    if (r.status === "error") {
      pushToast("err", r.error);
      term.writeln(`\x1b[31m${r.error}\x1b[0m`);
      return;
    }
    if (disposed) {
      // Component was torn down while opening — close the orphan session.
      void commands.termClose(r.data);
      return;
    }
    backendId = r.data;

    // Input → PTY.
    const onData = term.onData((d) => {
      if (backendId) void commands.termWrite(backendId, Array.from(new TextEncoder().encode(d)));
    });
    cleanups.push(() => onData.dispose());

    // PTY output → terminal (filtered by our session id).
    const unOut = await events.termOutputEvent.listen((e) => {
      if (e.payload.id === backendId) term.write(new Uint8Array(e.payload.data));
    });
    const unExit = await events.termExitEvent.listen((e) => {
      if (e.payload.id === backendId) {
        term.writeln("\r\n\x1b[90m[session closed]\x1b[0m");
        backendId = null;
      }
    });
    cleanups.push(unOut, unExit);

    // Keep the PTY size in sync with the widget.
    const ro = new ResizeObserver(() => {
      try {
        fit.fit();
        if (backendId) void commands.termResize(backendId, term.cols, term.rows);
      } catch {
        /* element not measurable yet */
      }
    });
    ro.observe(el);
    cleanups.push(() => ro.disconnect());

    term.focus();
  });

  onDestroy(() => {
    disposed = true;
    for (const c of cleanups) c();
    if (backendId) void commands.termClose(backendId);
    term?.dispose();
  });
</script>

<div class="term" bind:this={el}></div>

<style>
  .term {
    width: 100%;
    height: 100%;
    background: #0b0f18;
    /* Extra bottom room so the last row never sits flush against the window
       edge (the fit addon rounds rows down to this padded height). */
    padding: 8px 12px 20px;
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
