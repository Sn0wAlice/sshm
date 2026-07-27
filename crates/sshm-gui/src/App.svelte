<script lang="ts">
  import { onMount } from "svelte";
  import { commands, events } from "./lib/ipc";
  import { hosts, folders, activeSection, activeView, sessions, pushToast } from "./lib/stores";
  import Sidebar from "./lib/components/Sidebar.svelte";
  import TopTabs from "./lib/components/TopTabs.svelte";
  import Toasts from "./lib/components/Toasts.svelte";
  import HostsTab from "./lib/components/HostsTab.svelte";
  import TunnelsTab from "./lib/components/TunnelsTab.svelte";
  import KlusterTab from "./lib/components/KlusterTab.svelte";
  import IdentitiesTab from "./lib/components/IdentitiesTab.svelte";
  import SettingsTab from "./lib/components/SettingsTab.svelte";
  import Terminal from "./lib/components/Terminal.svelte";

  async function refreshHosts(): Promise<void> {
    hosts.set(await commands.listHosts(null));
    folders.set(await commands.listFolders());
  }

  onMount(() => {
    refreshHosts();
    const unlisten = events.dbChangedEvent.listen((e) => {
      if (e.payload.hosts) {
        refreshHosts();
        pushToast("ok", "Hosts reloaded (changed on disk)");
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  });

  $: sessionList = $sessions;
</script>

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
