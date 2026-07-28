<script lang="ts">
  import { onMount } from "svelte";
  import type { AppConfig } from "../bindings";
  import { commands } from "../ipc";
  import { pushToast } from "../stores";
  import Switch from "./Switch.svelte";

  let cfg: AppConfig | null = null;
  let saved = false;
  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  let savedTimer: ReturnType<typeof setTimeout> | undefined;

  onMount(async () => {
    cfg = await commands.getSettings();
  });

  // Save on change, debounced, with a subtle "Saved" confirmation.
  function scheduleSave(): void {
    if (!cfg) return;
    clearTimeout(saveTimer);
    saveTimer = setTimeout(async () => {
      if (!cfg) return;
      const r = await commands.saveSettings(cfg);
      if (r.status === "error") {
        pushToast("err", r.error);
        return;
      }
      saved = true;
      clearTimeout(savedTimer);
      savedTimer = setTimeout(() => (saved = false), 1600);
    }, 450);
  }
</script>

<div class="wrap">
  <div class="head">
    <h2>Settings</h2>
    <span class="saved" class:show={saved}>Saved ✓</span>
  </div>

  {#if cfg}
    <section>
      <div class="s-title">General</div>
      <div class="grid">
        <label>Default port<input type="number" bind:value={cfg.default_port} on:input={scheduleSave} /></label>
        <label>Default user<input bind:value={cfg.default_username} on:input={scheduleSave} /></label>
        <label>Default identity file<input bind:value={cfg.default_identity_file} on:input={scheduleSave} /></label>
        <label>
          External terminal <span class="hint">empty = auto-detect</span>
          <input bind:value={cfg.external_terminal} on:input={scheduleSave} placeholder="kitty -e / wezterm start --" />
        </label>
      </div>
    </section>

    <section>
      <div class="s-title">Export</div>
      <label>~/.ssh/config export path<input bind:value={cfg.export_path} on:input={scheduleSave} placeholder="~/.ssh/config" /></label>
    </section>

    <section>
      <div class="s-title">Health checks</div>
      <div class="toggle">
        <div><div class="t-label">Auto health-check hosts</div><div class="hint">Probe reachability in the background</div></div>
        <Switch bind:checked={cfg.auto_health_check} on:change={scheduleSave} />
      </div>
      <div class="toggle">
        <div><div class="t-label">Pause during a session</div><div class="hint">Stop probing while you're connected</div></div>
        <Switch bind:checked={cfg.pause_health_on_session} on:change={scheduleSave} />
      </div>
      <div class="grid">
        <label>Re-probe interval (s)<input type="number" bind:value={cfg.health_ttl_secs} on:input={scheduleSave} /></label>
        <label>Probe timeout (ms)<input type="number" bind:value={cfg.health_probe_timeout_ms} on:input={scheduleSave} /></label>
      </div>
    </section>

    <section>
      <div class="s-title">Kluster</div>
      <div class="grid">
        <label>Refresh interval (s)<input type="number" bind:value={cfg.kluster_refresh_secs} on:input={scheduleSave} /></label>
        <label>Log tail lines<input type="number" bind:value={cfg.kluster_log_tail_lines} on:input={scheduleSave} /></label>
      </div>
    </section>

    <section>
      <div class="s-title">Notifications</div>
      <div class="toggle">
        <div><div class="t-label">Desktop notifications</div><div class="hint">Tunnel dropped, host up/down…</div></div>
        <Switch bind:checked={cfg.notifications_enabled} on:change={scheduleSave} />
      </div>
      <label>Custom notification icon<input bind:value={cfg.notification_icon} on:input={scheduleSave} placeholder="~/path/to/icon.png" /></label>
    </section>
  {:else}
    <p class="muted">Loading…</p>
  {/if}
</div>

<style>
  .wrap {
    padding: 22px 24px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 26px;
    max-width: 760px;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  h2 {
    font-size: 18px;
  }
  .saved {
    font-size: 12px;
    color: var(--ok);
    opacity: 0;
    transition: opacity 0.2s ease;
  }
  .saved.show {
    opacity: 1;
  }
  section {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .s-title {
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.6px;
    color: var(--fg-dim);
    padding-bottom: 4px;
    border-bottom: 1px solid var(--border-soft);
  }
  .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 5px;
    font-size: 12px;
    color: var(--fg-dim);
  }
  .hint {
    color: var(--fg-faint);
    font-weight: 400;
  }
  .toggle {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 10px 12px;
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: 10px;
  }
  .t-label {
    color: var(--fg);
    font-size: 13.5px;
    font-weight: 500;
  }
</style>
