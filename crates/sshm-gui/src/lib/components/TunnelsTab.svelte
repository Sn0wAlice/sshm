<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import type { TunnelRecord, Host, Tunnel } from "../bindings";
  import { commands, tryRun } from "../ipc";
  import { hosts } from "../stores";

  let active: TunnelRecord[] = [];
  let timer: ReturnType<typeof setInterval> | undefined;

  async function refresh(): Promise<void> {
    active = await commands.listTunnels();
  }

  onMount(() => {
    refresh();
    timer = setInterval(refresh, 2000);
  });
  onDestroy(() => clearInterval(timer));

  async function start(h: Host, index: number): Promise<void> {
    await tryRun(commands.startTunnel(h.name, index), "Tunnel started");
    await refresh();
  }
  async function stop(pid: number): Promise<void> {
    await tryRun(commands.stopTunnel(pid), "Tunnel stopped");
    await refresh();
  }

  function fwd(t: Tunnel): string {
    if (t.kind === "Dynamic") return `SOCKS :${t.local_port}`;
    const rh = t.remote_host || "localhost";
    const arrow = t.kind === "Local" ? "→" : "←";
    return `:${t.local_port} ${arrow} ${rh}:${t.remote_port}`;
  }

  $: withTunnels = $hosts.filter((h) => (h.tunnels ?? []).length > 0);
</script>

<div class="wrap">
  <section>
    <h2>Active tunnels <span class="muted">({active.length})</span></h2>
    {#if active.length === 0}
      <p class="muted">No background tunnels running (across any sshm instance).</p>
    {:else}
      <div class="list">
        {#each active as t (t.pid)}
          <div class="card row">
            <div class="col grow">
              <strong>{t.host_name} <span class="tag">{t.tunnel.kind}</span></strong>
              <span class="mono muted">{fwd(t.tunnel)} · {t.host_display}</span>
            </div>
            <span class="mono muted">pid {t.pid}</span>
            <button class="danger" on:click={() => stop(t.pid)}>Stop</button>
          </div>
        {/each}
      </div>
    {/if}
  </section>

  <section>
    <h2>Saved tunnels</h2>
    {#if withTunnels.length === 0}
      <p class="muted">No host has saved tunnels. Add them when editing a host.</p>
    {:else}
      {#each withTunnels as h (h.name)}
        <div class="card col">
          <strong>{h.name}</strong>
          {#each h.tunnels ?? [] as t, i}
            <div class="row">
              <span class="mono">{t.label || "(unnamed)"} — {fwd(t)}</span>
              <div class="spacer"></div>
              <button on:click={() => start(h, i)}>Start</button>
            </div>
          {/each}
        </div>
      {/each}
    {/if}
  </section>
</div>

<style>
  .wrap {
    padding: 18px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 24px;
  }
  h2 {
    margin: 0 0 10px;
  }
  .list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .card {
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 10px 14px;
    gap: 10px;
  }
  .grow {
    flex: 1;
  }
</style>
