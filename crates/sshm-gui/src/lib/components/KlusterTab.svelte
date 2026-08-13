<script lang="ts">
  import { onMount } from "svelte";
  import type { ContainerInfo, IncusInstance, PodInfo, LifecycleAction } from "../bindings";
  import { commands, tryRun, openBackendSession } from "../ipc";
  import {
    klSources,
    klCurrentId,
    klCache,
    klError,
    klRefreshing,
    klDiscovering,
    klDetail,
    selectSource,
    refreshCurrent,
    loadOverview,
    ensureKlusterLoaded,
    incusDetail,
    podDetail,
  } from "../kluster";

  // Show whatever is cached immediately, then refresh in the background.
  onMount(ensureKlusterLoaded);

  $: current = $klSources.find((s) => s.id === $klCurrentId) ?? null;
  $: entry = ($klCurrentId && $klCache[$klCurrentId]) || {};
  $: containers = (entry.containers ?? []) as ContainerInfo[];
  $: pods = (entry.pods ?? []) as PodInfo[];
  $: instances = (entry.instances ?? []) as IncusInstance[];
  // "Loading" only when we have nothing to show yet: discovering runtimes, or
  // fetching a source we have no cache for.
  $: firstLoad =
    ($klDiscovering && !current) || ($klRefreshing && !$klCache[$klCurrentId ?? ""]);

  async function containerLife(c: ContainerInfo, action: LifecycleAction): Promise<void> {
    if (current?.kind === "docker")
      await tryRun(commands.klusterDockerLifecycle(c.id, action, current.hostAlias));
    else if (current?.kind === "apple")
      await tryRun(commands.klusterAppleLifecycle(c.id, action));
    refreshCurrent();
  }
  function containerShell(c: ContainerInfo): void {
    if (current?.kind === "docker")
      openBackendSession(commands.klusterDockerShell(c.id, current.hostAlias), `sh · ${c.name}`);
    else if (current?.kind === "apple")
      openBackendSession(commands.klusterAppleShell(c.id), `sh · ${c.name}`);
  }
  function containerLogs(c: ContainerInfo): void {
    if (current?.kind === "docker")
      openBackendSession(commands.klusterDockerLogs(c.id, current.hostAlias), `logs · ${c.name}`);
    else if (current?.kind === "apple")
      openBackendSession(commands.klusterAppleLogs(c.id), `logs · ${c.name}`);
  }
  function containerInspect(c: ContainerInfo): void {
    if (current?.kind === "docker")
      klDetail.set({ title: c.name, fetch: () => commands.klusterDockerInspect(c.id, current!.hostAlias) });
    else if (current?.kind === "apple")
      klDetail.set({ title: c.name, fetch: () => commands.klusterAppleInspect(c.id) });
  }

  async function incusLife(inst: IncusInstance, action: LifecycleAction): Promise<void> {
    await tryRun(commands.klusterIncusLifecycle(inst.name, action, current?.remote ?? null));
    refreshCurrent();
  }
  function incusInspect(inst: IncusInstance): void {
    klDetail.set({ title: inst.name, detail: incusDetail(inst, current?.remote ?? null) });
  }
  function podInspect(p: PodInfo): void {
    if (current?.cluster) klDetail.set({ title: p.name, detail: podDetail(p, current.cluster) });
  }
</script>

<div class="kl">
  <aside class="sources scroll">
    <button on:click={loadOverview} class="refresh" disabled={$klRefreshing || $klDiscovering}>↻ Refresh</button>
    {#each $klSources as s (s.id)}
      <button class="src" class:sel={$klCurrentId === s.id} on:click={() => selectSource(s.id)}>{s.label}</button>
    {/each}
    {#if $klSources.length === 0}
      {#if $klDiscovering}
        <div class="detecting"><div class="spinner"></div><span class="muted small">Detecting runtimes…</span></div>
      {:else}
        <p class="muted small">No Docker daemon, Apple container, cluster, or Incus found.</p>
      {/if}
    {/if}
  </aside>

  <div class="items scroll">
    {#if $klRefreshing && !firstLoad}
      <div class="refreshing">Refreshing…</div>
    {/if}

    {#if firstLoad}
      <p class="muted">Loading…</p>
    {:else if $klError}
      <p class="err">{$klError}</p>
    {:else if current?.kind === "docker" || current?.kind === "apple"}
      {#each containers as c (c.id)}
        <div class="card row">
          <span class="state" class:up={c.running}></span>
          <div class="col grow">
            <strong>{c.name}</strong>
            <span class="mono muted small">{c.image} · {c.status}</span>
          </div>
          <button on:click={() => containerInspect(c)}>Inspect</button>
          <button on:click={() => containerLife(c, c.running ? "Stop" : "Start")}>{c.running ? "Stop" : "Start"}</button>
          <button on:click={() => containerLife(c, "Restart")}>Restart</button>
          <button on:click={() => containerShell(c)}>Shell</button>
          <button on:click={() => containerLogs(c)}>Logs</button>
        </div>
      {:else}
        <p class="muted">No containers.</p>
      {/each}
    {:else if current?.kind === "pods"}
      {#each pods as p (p.namespace + "/" + p.name)}
        <div class="card row">
          <div class="col grow">
            <strong>{p.name}</strong>
            <span class="mono muted small">{p.namespace} · {p.phase} · {p.containers.join(", ")}</span>
          </div>
          <button on:click={() => podInspect(p)}>Inspect</button>
          {#if current?.cluster}
            <button on:click={() => current?.cluster && openBackendSession(commands.klusterPodShell(current.cluster, p.namespace, p.name), `sh · ${p.name}`)}>Shell</button>
          {/if}
        </div>
      {:else}
        <p class="muted">No pods.</p>
      {/each}
    {:else if current?.kind === "incus"}
      {#each instances as inst (inst.name)}
        <div class="card row">
          <span class="state" class:up={inst.running}></span>
          <div class="col grow">
            <strong>{inst.name}</strong>
            <span class="mono muted small">{inst.kind} · {inst.status} · {inst.image}</span>
          </div>
          <button on:click={() => incusInspect(inst)}>Inspect</button>
          <button on:click={() => incusLife(inst, inst.running ? "Stop" : "Start")}>{inst.running ? "Stop" : "Start"}</button>
          <button on:click={() => incusLife(inst, "Restart")}>Restart</button>
          <button on:click={() => openBackendSession(commands.klusterIncusShell(inst.name, current?.remote ?? null), `sh · ${inst.name}`)}>Shell</button>
        </div>
      {:else}
        <p class="muted">No instances.</p>
      {/each}
    {:else}
      <p class="muted">Select a source on the left.</p>
    {/if}
  </div>
</div>

<style>
  .kl {
    display: grid;
    grid-template-columns: 220px 1fr;
    height: 100%;
    min-height: 0;
  }
  .sources {
    border-right: 1px solid var(--border);
    padding: 10px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .refresh {
    margin-bottom: 6px;
  }
  .src {
    text-align: left;
    background: transparent;
    border: none;
    border-radius: var(--radius);
    padding: 7px 10px;
  }
  .src:hover {
    background: var(--bg-2);
  }
  .src.sel {
    background: var(--bg-3);
    color: var(--accent);
  }
  .items {
    padding: 14px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .refreshing {
    font-size: 11px;
    color: var(--fg-dim);
    align-self: flex-end;
    margin-bottom: -2px;
  }
  .detecting {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 4px;
  }
  .spinner {
    width: 16px;
    height: 16px;
    border-radius: 50%;
    border: 2px solid var(--border);
    border-top-color: var(--accent);
    animation: spin 0.7s linear infinite;
    flex: none;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .card {
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 8px 12px;
    gap: 8px;
  }
  .grow {
    flex: 1;
  }
  .small {
    font-size: 12px;
  }
  .state {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: var(--fg-dim);
    flex: none;
  }
  .state.up {
    background: var(--ok);
  }
  .err {
    color: var(--danger);
  }
</style>
