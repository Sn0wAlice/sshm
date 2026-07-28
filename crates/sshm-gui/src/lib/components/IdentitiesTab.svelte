<script lang="ts">
  import { onMount } from "svelte";
  import type { IdentityDto } from "../bindings";
  import { commands, tryRun } from "../ipc";
  import { hosts } from "../stores";
  import { promptDialog } from "../dialogs";

  let keys: IdentityDto[] = [];
  let genOpen = false;
  let genType = "ed25519";
  let genName = "id_ed25519";
  let genComment = "";
  let genPass = "";

  onMount(refresh);
  async function refresh(): Promise<void> {
    keys = await commands.listIdentities();
  }

  async function toggleAgent(k: IdentityDto): Promise<void> {
    if (k.in_agent) await tryRun(commands.agentRemoveIdentity(k.private), "Removed from agent");
    else await tryRun(commands.agentAddIdentity(k.private), "Added to agent");
    await refresh();
  }

  async function push(k: IdentityDto): Promise<void> {
    const host = await promptDialog({
      title: "Push public key to a host",
      message: "Adds this key to the host's authorized_keys.",
      placeholder: "Host name",
      confirmLabel: "Push",
    });
    if (!host) return;
    await tryRun(commands.pushPubkey(host, k.public), `Key pushed to ${host}`);
  }

  async function generate(): Promise<void> {
    const ok = await tryRun(
      commands.generateIdentity(genType, genName, genComment, genPass),
      `Generated ${genName}`,
    );
    if (ok !== undefined) {
      genOpen = false;
      genPass = "";
      await refresh();
    }
  }

  $: hostNames = $hosts.map((h) => h.name);
</script>

<div class="wrap">
  <div class="row head">
    <h2>SSH keys <span class="muted">(~/.ssh)</span></h2>
    <div class="spacer"></div>
    <button on:click={refresh}>↻</button>
    <button class="primary" on:click={() => (genOpen = !genOpen)}>Generate key</button>
  </div>

  {#if genOpen}
    <div class="card gen">
      <div class="row">
        <label>Type
          <select bind:value={genType}>
            <option value="ed25519">ed25519</option>
            <option value="ecdsa">ecdsa</option>
            <option value="rsa">rsa (4096)</option>
          </select>
        </label>
        <label>Filename<input bind:value={genName} /></label>
        <label>Comment<input bind:value={genComment} placeholder="me@laptop" /></label>
        <label>Passphrase<input type="password" bind:value={genPass} /></label>
      </div>
      <div class="row">
        <div class="spacer"></div>
        <button on:click={() => (genOpen = false)}>Cancel</button>
        <button class="primary" on:click={generate}>Create</button>
      </div>
    </div>
  {/if}

  <div class="list">
    {#each keys as k (k.private)}
      <div class="card row">
        <div class="col grow">
          <strong>
            {k.private.split("/").pop()}
            <span class="tag">{k.key_type}{k.bits ? ` ${k.bits}` : ""}</span>
            {#if k.is_hardware}<span class="tag">🔐 hardware</span>{/if}
            {#if k.in_agent}<span class="tag agent">agent</span>{/if}
          </strong>
          <span class="mono muted small">{k.fingerprint}</span>
          {#if k.comment}<span class="muted small">{k.comment}</span>{/if}
        </div>
        <button on:click={() => toggleAgent(k)}>{k.in_agent ? "Unload" : "Load"}</button>
        <button on:click={() => push(k)} disabled={hostNames.length === 0}>Push→host</button>
      </div>
    {:else}
      <p class="muted">No keys found in ~/.ssh.</p>
    {/each}
  </div>
</div>

<style>
  .wrap {
    padding: 18px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .head h2 {
    margin: 0;
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
  .gen label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 12px;
    color: var(--fg-dim);
    flex: 1;
  }
  .grow {
    flex: 1;
  }
  .small {
    font-size: 12px;
  }
  .tag.agent {
    color: var(--ok);
    border-color: var(--ok);
  }
</style>
