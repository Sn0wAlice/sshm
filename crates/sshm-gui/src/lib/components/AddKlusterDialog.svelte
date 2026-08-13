<script lang="ts">
  import { fade, scale } from "svelte/transition";
  import type { ClusterKind } from "../bindings";
  import { commands } from "../ipc";
  import { hosts, pushToast } from "../stores";
  import { klAddOpen, klOverview, loadOverview, selectSource } from "../kluster";

  type Tab = "cluster" | "docker";
  let tab: Tab = "cluster";
  let busy = false;

  // Cluster fields.
  let name = "";
  let kind: ClusterKind = "K8s";
  let context = "";
  let kubeconfig = "";
  let namespace = "";
  let contexts: string[] = [];

  // Docker-host field.
  let hostAlias = "";

  // Hosts not already registered as Docker remotes.
  $: existingRemotes = new Set(($klOverview?.docker_remotes ?? []).map((r) => r.host_alias));
  $: eligibleHosts = $hosts.filter((h) => !existingRemotes.has(h.name));

  // Reset + load context suggestions each time the dialog opens.
  let wasOpen = false;
  $: if ($klAddOpen && !wasOpen) {
    wasOpen = true;
    reset();
    commands.klusterKubeContexts().then((c) => (contexts = c));
  } else if (!$klAddOpen) {
    wasOpen = false;
  }

  function reset(): void {
    tab = "cluster";
    name = "";
    kind = "K8s";
    context = "";
    kubeconfig = "";
    namespace = "";
    hostAlias = "";
  }

  function close(): void {
    klAddOpen.set(false);
  }

  async function save(): Promise<void> {
    busy = true;
    try {
      if (tab === "cluster") {
        // Fall back to the context name when no explicit name was given.
        const finalName = name.trim() || context.trim();
        if (!finalName) {
          pushToast("err", "Give the cluster a name (or pick a context)");
          return;
        }
        const r = await commands.klusterAddCluster({
          name: finalName,
          kind,
          kubeconfig: kubeconfig.trim() || null,
          context: context.trim() || null,
          namespace_default: namespace.trim() || null,
        });
        if (r.status !== "ok") {
          pushToast("err", r.error);
          return;
        }
        pushToast("ok", `Added cluster ${finalName}`);
        await loadOverview();
        selectSource(`k8s-${finalName}`);
      } else {
        if (!hostAlias) {
          pushToast("err", "Pick a saved host");
          return;
        }
        const r = await commands.klusterAddDockerRemote(hostAlias);
        if (r.status !== "ok") {
          pushToast("err", r.error);
          return;
        }
        pushToast("ok", `Added Docker host ${hostAlias}`);
        await loadOverview();
        selectSource(`docker-${hostAlias}`);
      }
      close();
    } finally {
      busy = false;
    }
  }

  function onKey(e: KeyboardEvent): void {
    if (e.key === "Escape") {
      e.preventDefault();
      close();
    }
  }
</script>

<svelte:window on:keydown={onKey} />

{#if $klAddOpen}
  <div class="backdrop" transition:fade={{ duration: 120 }} on:click|self={close} role="presentation">
    <div class="modal" transition:scale={{ duration: 140, start: 0.97 }}>
      <div class="title">Add a Kluster source</div>

      <div class="tabs">
        <button class:sel={tab === "cluster"} on:click={() => (tab = "cluster")}>Kubernetes cluster</button>
        <button class:sel={tab === "docker"} on:click={() => (tab = "docker")}>Docker host (SSH)</button>
      </div>

      {#if tab === "cluster"}
        <label>Name<input bind:value={name} placeholder="prod-eks" /></label>
        <label>Type
          <select bind:value={kind}>
            <option value="K8s">k8s</option>
            <option value="K3s">k3s</option>
          </select>
        </label>
        <label>Context
          <input bind:value={context} list="kube-contexts" placeholder="(kubeconfig current-context)" />
        </label>
        <datalist id="kube-contexts">
          {#each contexts as c}<option value={c}></option>{/each}
        </datalist>
        <label>Kubeconfig path <span class="opt">optional</span>
          <input bind:value={kubeconfig} placeholder="~/.kube/config" />
        </label>
        <label>Default namespace <span class="opt">optional</span>
          <input bind:value={namespace} placeholder="default" />
        </label>
      {:else}
        {#if eligibleHosts.length}
          <label>Host running Docker
            <select bind:value={hostAlias}>
              <option value="" disabled selected>Pick a saved host…</option>
              {#each eligibleHosts as h}
                <option value={h.name}>{h.name} ({h.username}@{h.host}:{h.port})</option>
              {/each}
            </select>
          </label>
          <p class="hint">sshm sets <span class="mono">DOCKER_HOST=ssh://…</span> and tunnels natively — no port to open.</p>
        {:else}
          <p class="hint">No eligible host. Add an SSH host in the Hosts tab first (existing Docker remotes are hidden).</p>
        {/if}
      {/if}

      <div class="actions">
        <button on:click={close}>Cancel</button>
        <button class="primary" on:click={save} disabled={busy || (tab === "docker" && !eligibleHosts.length)}>Add</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.55);
    backdrop-filter: blur(2px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 80;
  }
  .modal {
    width: min(440px, 94vw);
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: 14px;
    padding: 20px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    box-shadow: 0 24px 70px rgba(0, 0, 0, 0.6);
  }
  .title {
    font-size: 15px;
    font-weight: 650;
  }
  .tabs {
    display: flex;
    gap: 6px;
  }
  .tabs button {
    flex: 1;
    background: var(--bg-1);
  }
  .tabs button.sel {
    background: var(--bg-3);
    color: var(--accent);
    border-color: var(--accent);
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 12.5px;
    color: var(--fg-dim);
  }
  input,
  select {
    font-size: 13px;
  }
  .opt {
    color: var(--fg-dim);
    font-size: 11px;
  }
  .hint {
    font-size: 12px;
    color: var(--fg-dim);
    margin: 0;
  }
  .mono {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 4px;
  }
</style>
