<script lang="ts">
  import { activeSection, activeView, type Section } from "../stores";
  import Icon from "./Icon.svelte";

  const items: { id: Section; label: string; icon: any }[] = [
    { id: "hosts", label: "Hosts", icon: "hosts" },
    { id: "portforward", label: "Port forwarding", icon: "portforward" },
    { id: "kluster", label: "Kluster", icon: "kluster" },
    { id: "keychain", label: "Keychain", icon: "keychain" },
    { id: "settings", label: "Settings", icon: "settings" },
  ];

  function go(s: Section): void {
    activeSection.set(s);
    activeView.set("manager");
  }
</script>

<nav>
  <div class="logo"><span class="mark">◈</span> sshm</div>
  {#each items as it}
    <button
      class="nav"
      class:active={$activeView === "manager" && $activeSection === it.id}
      on:click={() => go(it.id)}
    >
      <Icon name={it.icon} size={18} />
      <span>{it.label}</span>
    </button>
  {/each}
  <div class="spacer"></div>
  <div class="foot">shared vault<br /><span class="mono">~/.config/sshm</span></div>
</nav>

<style>
  nav {
    width: 178px;
    background: var(--bg-0);
    border-right: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    padding: 14px 10px;
    gap: 3px;
  }
  .logo {
    font-weight: 800;
    font-size: 17px;
    padding: 4px 10px 16px;
    letter-spacing: 0.4px;
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .mark {
    color: var(--accent);
  }
  button.nav {
    position: relative;
    display: flex;
    align-items: center;
    gap: 11px;
    background: transparent;
    border: none;
    color: var(--fg-dim);
    text-align: left;
    padding: 9px 10px;
    border-radius: 9px;
    font-size: 13.5px;
  }
  button.nav:hover {
    background: var(--bg-2);
    color: var(--fg);
  }
  button.nav.active {
    background: var(--bg-3);
    color: var(--fg);
    font-weight: 600;
  }
  button.nav.active::before {
    content: "";
    position: absolute;
    left: -10px;
    top: 50%;
    transform: translateY(-50%);
    width: 3px;
    height: 18px;
    border-radius: 0 3px 3px 0;
    background: var(--fg);
  }
  .spacer {
    flex: 1;
  }
  .foot {
    font-size: 11px;
    color: var(--fg-faint);
    padding: 10px;
    line-height: 1.5;
  }
</style>
