<script lang="ts">
  import { onMount } from "svelte";
  import { commands, events } from "./lib/ipc";
  import { activeTab, hosts, folders, pushToast } from "./lib/stores";
  import Sidebar from "./lib/components/Sidebar.svelte";
  import Toasts from "./lib/components/Toasts.svelte";
  import HostsTab from "./lib/components/HostsTab.svelte";
  import TunnelsTab from "./lib/components/TunnelsTab.svelte";
  import KlusterTab from "./lib/components/KlusterTab.svelte";
  import IdentitiesTab from "./lib/components/IdentitiesTab.svelte";
  import SettingsTab from "./lib/components/SettingsTab.svelte";

  async function refreshHosts(): Promise<void> {
    hosts.set(await commands.listHosts(null));
    folders.set(await commands.listFolders());
  }

  onMount(() => {
    refreshHosts();
    // Live sync: the backend bridges sshm_core's file watcher to this event.
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
</script>

<div class="shell">
  <Sidebar />
  <main class="content">
    {#if $activeTab === "hosts"}
      <HostsTab />
    {:else if $activeTab === "tunnels"}
      <TunnelsTab />
    {:else if $activeTab === "kluster"}
      <KlusterTab />
    {:else if $activeTab === "identities"}
      <IdentitiesTab />
    {:else if $activeTab === "settings"}
      <SettingsTab />
    {/if}
  </main>
  <Toasts />
</div>

<style>
  .shell {
    display: grid;
    grid-template-columns: 180px 1fr;
    height: 100vh;
  }
  .content {
    overflow: hidden;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
</style>
