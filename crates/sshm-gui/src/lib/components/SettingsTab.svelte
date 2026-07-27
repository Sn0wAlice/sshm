<script lang="ts">
  import { onMount } from "svelte";
  import type { AppConfig } from "../bindings";
  import { commands, tryRun } from "../ipc";

  let cfg: AppConfig | null = null;

  onMount(async () => {
    cfg = await commands.getSettings();
  });

  async function save(): Promise<void> {
    if (!cfg) return;
    await tryRun(commands.saveSettings(cfg), "Settings saved");
  }
</script>

<div class="wrap">
  <h2>Settings</h2>
  {#if cfg}
    <div class="grid">
      <label>Default port<input type="number" bind:value={cfg.default_port} /></label>
      <label>Default user<input bind:value={cfg.default_username} /></label>
      <label>Default identity file<input bind:value={cfg.default_identity_file} /></label>
      <label>Export path (~/.ssh/config)<input bind:value={cfg.export_path} /></label>
      <label>
        External terminal (empty = auto)
        <input bind:value={cfg.external_terminal} placeholder="kitty -e / wezterm start --" />
      </label>
      <label>Notification icon<input bind:value={cfg.notification_icon} /></label>
      <label>Health re-probe (s)<input type="number" bind:value={cfg.health_ttl_secs} /></label>
      <label>Health timeout (ms)<input type="number" bind:value={cfg.health_probe_timeout_ms} /></label>
      <label>Kluster refresh (s)<input type="number" bind:value={cfg.kluster_refresh_secs} /></label>
      <label>Kluster log tail<input type="number" bind:value={cfg.kluster_log_tail_lines} /></label>
    </div>

    <div class="checks col">
      <label class="check"><input type="checkbox" bind:checked={cfg.auto_health_check} /> Auto health-check hosts</label>
      <label class="check"><input type="checkbox" bind:checked={cfg.pause_health_on_session} /> Pause health checks during a session</label>
      <label class="check"><input type="checkbox" bind:checked={cfg.notifications_enabled} /> Desktop notifications</label>
    </div>

    <div class="row">
      <div class="spacer"></div>
      <button class="primary" on:click={save}>Save settings</button>
    </div>
  {:else}
    <p class="muted">Loading…</p>
  {/if}
</div>

<style>
  .wrap {
    padding: 18px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 16px;
    max-width: 720px;
  }
  h2 {
    margin: 0;
  }
  .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 12px;
    color: var(--fg-dim);
  }
  label.check {
    flex-direction: row;
    align-items: center;
    gap: 8px;
    color: var(--fg);
  }
  label.check input {
    width: auto;
  }
</style>
