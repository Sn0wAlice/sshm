<script lang="ts">
  import { activeTab, type Tab } from "../stores";

  const tabs: { id: Tab; label: string; icon: string }[] = [
    { id: "hosts", label: "Hosts", icon: "🖥" },
    { id: "tunnels", label: "Tunnels", icon: "🔀" },
    { id: "kluster", label: "Kluster", icon: "📦" },
    { id: "identities", label: "Identities", icon: "🔑" },
    { id: "settings", label: "Settings", icon: "⚙" },
  ];
</script>

<nav>
  <div class="brand">sshm</div>
  {#each tabs as t}
    <button
      class="nav"
      class:active={$activeTab === t.id}
      on:click={() => activeTab.set(t.id)}
    >
      <span class="icon">{t.icon}</span>
      {t.label}
    </button>
  {/each}
  <div class="spacer"></div>
  <div class="foot muted">shared DB · ~/.config/sshm</div>
</nav>

<style>
  nav {
    background: var(--bg-2);
    border-right: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    padding: 12px 8px;
    gap: 4px;
  }
  .brand {
    font-weight: 700;
    font-size: 18px;
    padding: 6px 10px 14px;
    letter-spacing: 0.5px;
  }
  button.nav {
    background: transparent;
    border: none;
    text-align: left;
    border-radius: var(--radius);
    padding: 8px 10px;
    display: flex;
    gap: 10px;
    align-items: center;
  }
  button.nav:hover {
    background: var(--bg-3);
  }
  button.nav.active {
    background: var(--bg-3);
    color: var(--accent);
    font-weight: 600;
  }
  .icon {
    width: 18px;
    text-align: center;
  }
  .spacer {
    flex: 1;
  }
  .foot {
    font-size: 11px;
    padding: 8px 10px;
  }
</style>
