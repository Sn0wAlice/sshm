<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import { fade, scale } from "svelte/transition";
  import type { Host, Tunnel, TunnelKind } from "../bindings";
  import { commands, tryRun } from "../ipc";

  export let host: Host | null = null;
  export let folders: string[] = [];

  const dispatch = createEventDispatcher<{ saved: void; cancel: void }>();

  const original = host?.name ?? null;

  // Editable model (defaults for a new host).
  let name = host?.name ?? "";
  let hostname = host?.host ?? "";
  let port = host?.port ?? 22;
  let username = host?.username ?? "root";
  let identity_file = host?.identity_file ?? "";
  let proxy_jump = host?.proxy_jump ?? "";
  let folder = host?.folder ?? "";
  let tagsText = (host?.tags ?? []).join(", ");
  let forward_agent = host?.forward_agent ?? false;
  let mosh = host?.mosh ?? false;
  let notes = host?.notes ?? "";
  let remote_command = host?.remote_command ?? "";
  let tunnels: Tunnel[] = (host?.tunnels ?? []).map((t) => ({ ...t }));

  const kinds: TunnelKind[] = ["Local", "Remote", "Dynamic"];

  function addTunnel(): void {
    tunnels = [
      ...tunnels,
      { label: "", kind: "Local", local_port: 0, remote_port: 0, remote_host: "" },
    ];
  }
  function removeTunnel(i: number): void {
    tunnels = tunnels.filter((_, idx) => idx !== i);
  }

  async function save(): Promise<void> {
    if (!name.trim() || !hostname.trim()) return;
    const tags = tagsText
      .split(",")
      .map((s) => s.trim())
      .filter(Boolean);
    const payload: Host = {
      name: name.trim(),
      host: hostname.trim(),
      port,
      username: username.trim() || "root",
      identity_file: identity_file.trim() || null,
      proxy_jump: proxy_jump.trim() || null,
      tags: tags.length ? tags : null,
      folder: folder.trim() || null,
      last_connected_at: host?.last_connected_at ?? null,
      use_count: host?.use_count ?? 0,
      favorite: host?.favorite ?? false,
      tunnels,
      forward_agent,
      mosh,
      notes: notes.trim() || null,
      remote_command: remote_command.trim() || null,
    };
    const ok = await tryRun(
      commands.saveHost(payload, original),
      original ? "Host updated" : "Host created",
    );
    if (ok !== undefined) dispatch("saved");
  }
</script>

<div class="backdrop" transition:fade={{ duration: 120 }} on:click|self={() => dispatch("cancel")} role="presentation">
  <div class="modal col" transition:scale={{ duration: 150, start: 0.97 }}>
    <h3>{original ? `Edit ${original}` : "New host"}</h3>

    <div class="grid">
      <label>Name<input bind:value={name} placeholder="web-prod" /></label>
      <label>Host / IP<input bind:value={hostname} placeholder="10.0.0.5" /></label>
      <label>Port<input type="number" bind:value={port} min="1" max="65535" /></label>
      <label>User<input bind:value={username} /></label>
      <label
        >Identity file<input bind:value={identity_file} placeholder="~/.ssh/id_ed25519" /></label
      >
      <label>ProxyJump<input bind:value={proxy_jump} placeholder="bastion1,bastion2" /></label>
      <label>
        Folder
        <input list="folders" bind:value={folder} placeholder="Production" />
        <datalist id="folders">
          {#each folders as f}<option value={f}></option>{/each}
        </datalist>
      </label>
      <label>Tags<input bind:value={tagsText} placeholder="prod, eu" /></label>
    </div>

    <label>Notes<textarea rows="2" bind:value={notes}></textarea></label>
    <label
      >Run on connect<input bind:value={remote_command} placeholder="tmux attach || tmux" /></label
    >

    <div class="row">
      <label class="check"><input type="checkbox" bind:checked={forward_agent} /> Forward agent (-A)</label>
      <label class="check"><input type="checkbox" bind:checked={mosh} /> Use mosh</label>
    </div>

    <div class="tunnels">
      <div class="row">
        <strong>Saved tunnels</strong>
        <button on:click={addTunnel}>+ Add</button>
      </div>
      {#each tunnels as t, i}
        <div class="trow">
          <input class="lbl" bind:value={t.label} placeholder="label" />
          <select bind:value={t.kind}>
            {#each kinds as k}<option value={k}>{k}</option>{/each}
          </select>
          <input type="number" bind:value={t.local_port} placeholder="local" />
          {#if t.kind !== "Dynamic"}
            <input bind:value={t.remote_host} placeholder="remote host" />
            <input type="number" bind:value={t.remote_port} placeholder="remote port" />
          {/if}
          <button class="danger" on:click={() => removeTunnel(i)}>✕</button>
        </div>
      {/each}
    </div>

    <div class="row">
      <div class="spacer"></div>
      <button on:click={() => dispatch("cancel")}>Cancel</button>
      <button class="primary" on:click={save} disabled={!name.trim() || !hostname.trim()}
        >Save</button
      >
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 40;
  }
  .modal {
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 18px;
    width: min(680px, 92vw);
    max-height: 90vh;
    overflow-y: auto;
  }
  h3 {
    margin: 0 0 4px;
  }
  .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
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
    gap: 6px;
    color: var(--fg);
  }
  label.check input {
    width: auto;
  }
  .tunnels {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 10px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .trow {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .trow .lbl {
    max-width: 120px;
  }
</style>
