// Shared Kluster state — lives at module scope so it survives KlusterTab
// mount/unmount (the tab is rendered under an {#if} in App.svelte and is
// destroyed every time you leave the section). Keeping the overview, the
// selected source and a per-source result cache here means:
//   • entering the section shows the last data instantly (no flash of "Loading"),
//   • a fresh fetch runs in the background and swaps in when it lands,
//   • the initial discovery can be kicked off at app startup, not on click
//     (which was racy and left the page blank ~half the time).

import { get, writable } from "svelte/store";
import type {
  KlusterOverview,
  ContainerInfo,
  ContainerDetail,
  IncusInstance,
  PodInfo,
  Cluster,
  Result,
} from "./bindings";
import { commands } from "./ipc";

export type Kind = "docker" | "apple" | "pods" | "incus";

export interface Source {
  id: string;
  label: string;
  kind: Kind;
  hostAlias: string | null;
  remote: string | null;
  cluster: Cluster | null;
}

interface CacheEntry {
  containers?: ContainerInfo[];
  pods?: PodInfo[];
  instances?: IncusInstance[];
}

export const klOverview = writable<KlusterOverview | null>(null);
export const klSources = writable<Source[]>([]);
export const klCurrentId = writable<string | null>(null);
/** Per-source result cache, keyed by Source.id. Shown instantly on re-entry. */
export const klCache = writable<Record<string, CacheEntry>>({});
/** Error for the *current* source's last fetch (cleared on success). */
export const klError = writable<string>("");
/** A background fetch for the current source is in flight. */
export const klRefreshing = writable<boolean>(false);
/** The overview probe (docker/apple/incus/kube availability) is in flight. */
export const klDiscovering = writable<boolean>(false);

function buildSources(ov: KlusterOverview): Source[] {
  const s: Source[] = [];
  if (ov.docker_local_available)
    s.push({ id: "docker-local", label: "Docker (local)", kind: "docker", hostAlias: null, remote: null, cluster: null });
  if (ov.apple_local_available)
    s.push({ id: "apple-local", label: "Apple container", kind: "apple", hostAlias: null, remote: null, cluster: null });
  for (const r of ov.docker_remotes)
    s.push({ id: `docker-${r.host_alias}`, label: `Docker @ ${r.host_alias}`, kind: "docker", hostAlias: r.host_alias, remote: null, cluster: null });
  for (const c of ov.clusters)
    s.push({ id: `k8s-${c.name}`, label: `k8s · ${c.name}`, kind: "pods", hostAlias: null, remote: null, cluster: c });
  if (ov.incus_local_available)
    s.push({ id: "incus-local", label: "Incus (local)", kind: "incus", hostAlias: null, remote: null, cluster: null });
  for (const r of ov.incus_remotes)
    s.push({ id: `incus-${r}`, label: `Incus @ ${r}`, kind: "incus", hostAlias: null, remote: r, cluster: null });
  return s;
}

/** Fetch one source's items. Returns the cache entry, or an error string. */
async function fetchSource(src: Source): Promise<{ entry?: CacheEntry; error?: string }> {
  if (src.kind === "docker") {
    const r = await commands.klusterDockerContainers(src.hostAlias);
    return r.status === "ok" ? { entry: { containers: r.data } } : { error: r.error };
  }
  if (src.kind === "apple") {
    const r = await commands.klusterAppleContainers();
    return r.status === "ok" ? { entry: { containers: r.data } } : { error: r.error };
  }
  if (src.kind === "pods" && src.cluster) {
    const r = await commands.klusterPods(src.cluster);
    return r.status === "ok" ? { entry: { pods: r.data } } : { error: r.error };
  }
  if (src.kind === "incus") {
    const r = await commands.klusterIncus(src.remote);
    return r.status === "ok" ? { entry: { instances: r.data } } : { error: r.error };
  }
  return { entry: {} };
}

/** Refresh one source in the background, updating the cache when it lands. */
export async function refreshSource(src: Source): Promise<void> {
  const isCurrent = () => get(klCurrentId) === src.id;
  if (isCurrent()) {
    klRefreshing.set(true);
    klError.set("");
  }
  try {
    const { entry, error } = await fetchSource(src);
    if (entry) {
      // Merge so a successful fetch replaces stale data even if not current.
      klCache.update((c) => ({ ...c, [src.id]: entry }));
      if (isCurrent()) klError.set("");
    } else if (error && isCurrent()) {
      // Keep the last good cache; just surface the error.
      klError.set(error);
    }
  } finally {
    if (isCurrent()) klRefreshing.set(false);
  }
}

/** Select a source: switch to it instantly (cache) and refresh in the background. */
export function selectSource(id: string): void {
  klCurrentId.set(id);
  klError.set("");
  const src = get(klSources).find((s) => s.id === id);
  if (src) void refreshSource(src);
}

/** Refresh the currently selected source, if any. */
export function refreshCurrent(): void {
  const id = get(klCurrentId);
  const src = get(klSources).find((s) => s.id === id);
  if (src) void refreshSource(src);
}

/**
 * (Re)load the overview + source list. Preserves the current selection when it
 * still exists; otherwise selects the first source. Safe to call repeatedly —
 * it refreshes availability and the current source each time.
 */
export async function loadOverview(): Promise<void> {
  klDiscovering.set(true);
  let ov;
  try {
    ov = await commands.klusterOverview();
  } finally {
    klDiscovering.set(false);
  }
  klOverview.set(ov);
  const sources = buildSources(ov);
  klSources.set(sources);

  const cur = get(klCurrentId);
  if (cur && sources.some((s) => s.id === cur)) {
    refreshCurrent();
  } else if (sources.length) {
    selectSource(sources[0].id);
  } else {
    klCurrentId.set(null);
  }
}

// --- Inspect / detail view --------------------------------------------------

/**
 * A request to open the detail popup. Either a `fetch` thunk (Docker/Apple run
 * a real `inspect` shell-out) or a prebuilt `detail` (Incus/pods are shown
 * compactly from the list snapshot, no extra shell-out — mirrors the TUI).
 */
export type DetailRequest =
  | { title: string; fetch: () => Promise<Result<ContainerDetail, string>> }
  | { title: string; detail: ContainerDetail };

export const klDetail = writable<DetailRequest | null>(null);

/** Whether the "add a Kluster source" dialog is open. */
export const klAddOpen = writable(false);

/** Compact detail for an Incus instance, built from the list snapshot. */
export function incusDetail(inst: IncusInstance, remote: string | null): ContainerDetail {
  const rows: [string, string][] = [
    ["Name", inst.name],
    ["Kind", inst.kind],
    ["Status", inst.status],
    ["Image", inst.image],
    ["Remote", remote ?? "local"],
  ].filter((r): r is [string, string] => r[1].trim() !== "");
  return { title: inst.name, sections: [{ title: "Overview", rows }], log_tail: [] };
}

/** Compact detail for a k8s pod, built from the list snapshot. */
export function podDetail(pod: PodInfo, cluster: Cluster): ContainerDetail {
  const rows: [string, string][] = [
    ["Pod", pod.name],
    ["Namespace", pod.namespace],
    ["Phase", pod.phase],
    ["Cluster", cluster.name],
    ["Containers", pod.containers.join(", ")],
  ].filter((r): r is [string, string] => r[1].trim() !== "");
  return { title: pod.name, sections: [{ title: "Overview", rows }], log_tail: [] };
}

let bootstrapped = false;
/**
 * Ensure Kluster data is loading. On first call it does a full discovery; later
 * calls just refresh the overview + current source in the background so the
 * cached view stays warm. Kicked off at app startup and on each tab entry.
 */
export function ensureKlusterLoaded(): void {
  bootstrapped = true;
  void loadOverview();
}

/** Whether discovery has ever been kicked off (drives first-load messaging). */
export const klBootstrapped = (): boolean => bootstrapped;
