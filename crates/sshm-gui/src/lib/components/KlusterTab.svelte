<script lang="ts">
  import { onMount } from "svelte";
  import type {
    KlusterOverview,
    ContainerInfo,
    IncusInstance,
    PodInfo,
    Cluster,
    LifecycleAction,
  } from "../bindings";
  import { commands, tryRun, openBackendSession } from "../ipc";

  type Kind = "docker" | "pods" | "incus";
  interface Source {
    id: string;
    label: string;
    kind: Kind;
    hostAlias: string | null;
    remote: string | null;
    cluster: Cluster | null;
  }

  let overview: KlusterOverview | null = null;
  let sources: Source[] = [];
  let current: Source | null = null;

  let containers: ContainerInfo[] = [];
  let pods: PodInfo[] = [];
  let instances: IncusInstance[] = [];
  let loading = false;
  let error = "";

  onMount(load);

  async function load(): Promise<void> {
    overview = await commands.klusterOverview();
    const s: Source[] = [];
    if (overview.docker_local_available)
      s.push({ id: "docker-local", label: "Docker (local)", kind: "docker", hostAlias: null, remote: null, cluster: null });
    for (const r of overview.docker_remotes)
      s.push({ id: `docker-${r.host_alias}`, label: `Docker @ ${r.host_alias}`, kind: "docker", hostAlias: r.host_alias, remote: null, cluster: null });
    for (const c of overview.clusters)
      s.push({ id: `k8s-${c.name}`, label: `k8s · ${c.name}`, kind: "pods", hostAlias: null, remote: null, cluster: c });
    if (overview.incus_local_available)
      s.push({ id: "incus-local", label: "Incus (local)", kind: "incus", hostAlias: null, remote: null, cluster: null });
    for (const r of overview.incus_remotes)
      s.push({ id: `incus-${r}`, label: `Incus @ ${r}`, kind: "incus", hostAlias: null, remote: r, cluster: null });
    sources = s;
    if (!current && sources.length) select(sources[0]);
  }

  async function select(src: Source): Promise<void> {
    current = src;
    error = "";
    loading = true;
    containers = [];
    pods = [];
    instances = [];
    try {
      if (src.kind === "docker") {
        const r = await commands.klusterDockerContainers(src.hostAlias);
        if (r.status === "ok") containers = r.data;
        else error = r.error;
      } else if (src.kind === "pods" && src.cluster) {
        const r = await commands.klusterPods(src.cluster);
        if (r.status === "ok") pods = r.data;
        else error = r.error;
      } else if (src.kind === "incus") {
        const r = await commands.klusterIncus(src.remote);
        if (r.status === "ok") instances = r.data;
        else error = r.error;
      }
    } finally {
      loading = false;
    }
  }

  async function dockerLife(id: string, action: LifecycleAction): Promise<void> {
    await tryRun(commands.klusterDockerLifecycle(id, action, current?.hostAlias ?? null));
    if (current) await select(current);
  }
  async function incusLife(name: string, action: LifecycleAction): Promise<void> {
    await tryRun(commands.klusterIncusLifecycle(name, action, current?.remote ?? null));
    if (current) await select(current);
  }
</script>

<div class="kl">
  <aside class="sources scroll">
    <button on:click={load} class="refresh">↻ Refresh</button>
    {#each sources as s (s.id)}
      <button class="src" class:sel={current?.id === s.id} on:click={() => select(s)}>{s.label}</button>
    {/each}
    {#if sources.length === 0}
      <p class="muted small">No Docker daemon, cluster, or Incus found.</p>
    {/if}
  </aside>

  <div class="items scroll">
    {#if loading}
      <p class="muted">Loading…</p>
    {:else if error}
      <p class="err">{error}</p>
    {:else if current?.kind === "docker"}
      {#each containers as c (c.id)}
        <div class="card row">
          <span class="state" class:up={c.running}></span>
          <div class="col grow">
            <strong>{c.name}</strong>
            <span class="mono muted small">{c.image} · {c.status}</span>
          </div>
          <button on:click={() => dockerLife(c.id, c.running ? "Stop" : "Start")}>{c.running ? "Stop" : "Start"}</button>
          <button on:click={() => dockerLife(c.id, "Restart")}>Restart</button>
          <button on:click={() => openBackendSession(commands.klusterDockerShell(c.id, current?.hostAlias ?? null), `sh · ${c.name}`)}>Shell</button>
          <button on:click={() => openBackendSession(commands.klusterDockerLogs(c.id, current?.hostAlias ?? null), `logs · ${c.name}`)}>Logs</button>
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
          <button on:click={() => incusLife(inst.name, inst.running ? "Stop" : "Start")}>{inst.running ? "Stop" : "Start"}</button>
          <button on:click={() => incusLife(inst.name, "Restart")}>Restart</button>
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
