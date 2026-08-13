<script lang="ts">
  import { onMount } from "svelte";
  import { commands, events, openLocalSession } from "./lib/ipc";
  import {
    hosts,
    folders,
    hostsLoading,
    activeSection,
    activeView,
    sessions,
    paletteOpen,
    closeSession,
    closedSessions,
    pushToast,
    type Section,
  } from "./lib/stores";
  import Sidebar from "./lib/components/Sidebar.svelte";
  import TopTabs from "./lib/components/TopTabs.svelte";
  import Toasts from "./lib/components/Toasts.svelte";
  import DialogHost from "./lib/components/DialogHost.svelte";
  import HostKeyDialog from "./lib/components/HostKeyDialog.svelte";
  import KlusterDetailDialog from "./lib/components/KlusterDetailDialog.svelte";
  import CommandPalette from "./lib/components/CommandPalette.svelte";
  import { ensureKlusterLoaded } from "./lib/kluster";
  import HostsTab from "./lib/components/HostsTab.svelte";
  import TunnelsTab from "./lib/components/TunnelsTab.svelte";
  import KlusterTab from "./lib/components/KlusterTab.svelte";
  import IdentitiesTab from "./lib/components/IdentitiesTab.svelte";
  import SettingsTab from "./lib/components/SettingsTab.svelte";
  import Terminal from "./lib/components/Terminal.svelte";

  async function refreshHosts(): Promise<void> {
    try {
      // Hosts come straight from a JSON read (no external tools), so they load
      // fast and independently of the login-shell PATH recovery.
      hosts.set(await commands.listHosts(null));
      folders.set(await commands.listFolders());
    } finally {
      hostsLoading.set(false);
    }
  }

  onMount(() => {
    refreshHosts();
    const unDb = events.dbChangedEvent.listen((e) => {
      if (e.payload.hosts) {
        refreshHosts();
        pushToast("ok", "Hosts reloaded (changed on disk)");
      }
    });
    // Kluster discovery needs docker/kubectl/incus on PATH — which is only
    // recovered off-thread after launch. Warm the cache once that lands, so the
    // UI is instant and the data streams in a moment later.
    const unPath = events.pathReadyEvent.listen(() => ensureKlusterLoaded());
    // Mark a session's tab dot when its PTY child exits.
    const unExit = events.termExitEvent.listen((e) => {
      closedSessions.update((s) => new Set(s).add(e.payload.id));
    });
    return () => {
      unDb.then((fn) => fn());
      unPath.then((fn) => fn());
      unExit.then((fn) => fn());
    };
  });

  const SECTIONS: Section[] = ["hosts", "portforward", "kluster", "keychain", "settings"];

  // Global keyboard shortcuts (capture phase so they win over the terminal).
  function onGlobalKey(e: KeyboardEvent): void {
    const mod = e.metaKey || e.ctrlKey;
    if (!mod) return;
    const k = e.key.toLowerCase();
    if (k === "k") {
      e.preventDefault();
      paletteOpen.update((v) => !v);
    } else if (k === "t") {
      e.preventDefault();
      openLocalSession();
    } else if (k === "w") {
      if ($activeView !== "manager") {
        e.preventDefault();
        closeSession($activeView);
      }
    } else if (k >= "1" && k <= "5") {
      e.preventDefault();
      activeSection.set(SECTIONS[Number(k) - 1]);
      activeView.set("manager");
    }
  }

  $: sessionList = $sessions;
</script>

<svelte:window on:keydown|capture={onGlobalKey} />

<div class="app">
  <TopTabs />
  <div class="body">
    <Sidebar />
    <main class="content">
      <!-- Terminals stay mounted (hidden) while inactive so sessions survive tab switches. -->
      {#each sessionList as s (s.id)}
        <div class="pane" class:show={$activeView === s.id}>
          <Terminal backendId={s.backendId} />
        </div>
      {/each}

      <div class="pane" class:show={$activeView === "manager"}>
        {#if $activeSection === "hosts"}
          <HostsTab />
        {:else if $activeSection === "portforward"}
          <TunnelsTab />
        {:else if $activeSection === "kluster"}
          <KlusterTab />
        {:else if $activeSection === "keychain"}
          <IdentitiesTab />
        {:else if $activeSection === "settings"}
          <SettingsTab />
        {/if}
      </div>
    </main>
  </div>
  <Toasts />
  <CommandPalette />
  <DialogHost />
  <HostKeyDialog />
  <KlusterDetailDialog />
</div>

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
  }
  .body {
    display: flex;
    flex: 1;
    min-height: 0;
  }
  .content {
    position: relative;
    flex: 1;
    min-width: 0;
    background: var(--bg-1);
  }
  .pane {
    position: absolute;
    inset: 0;
    display: none;
    min-height: 0;
  }
  .pane.show {
    display: flex;
    flex-direction: column;
  }
</style>
