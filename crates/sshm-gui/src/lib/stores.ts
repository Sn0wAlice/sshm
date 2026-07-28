import { writable } from "svelte/store";
import type { Host } from "./bindings";

// Left-rail sections (the "manager" surface).
export type Section = "hosts" | "portforward" | "kluster" | "keychain" | "settings";
export const activeSection = writable<Section>("hosts");

// Top strip: the host manager plus one tab per open embedded terminal.
export interface Session {
  id: string; // frontend id (drives the tab / Terminal component)
  backendId: string; // the PTY session id in the Rust backend
  title: string;
}
export const sessions = writable<Session[]>([]);
/** "manager" shows the section views; a session id shows that terminal. */
export const activeView = writable<"manager" | string>("manager");
/** backendIds whose PTY child has exited (drives the tab status dot). */
export const closedSessions = writable<Set<string>>(new Set());

export const hosts = writable<Host[]>([]);
export const folders = writable<string[]>([]);
export const selectedHostName = writable<string | null>(null);

/** ⌘K command palette visibility. */
export const paletteOpen = writable(false);
/** Bumped to ask the Hosts view to open the New-host form (from the palette). */
export const newHostRequest = writable(0);

let sessionSeq = 0;
/** Open a tab for an already-spawned backend PTY session. */
export function addSession(backendId: string, title: string): string {
  const id = `t${++sessionSeq}`;
  sessions.update((s) => [...s, { id, backendId, title }]);
  activeView.set(id);
  return id;
}
export function closeSession(id: string): void {
  let remaining: Session[] = [];
  sessions.update((s) => {
    remaining = s.filter((x) => x.id !== id);
    return remaining;
  });
  activeView.update((v) => (v === id ? (remaining.length ? remaining[remaining.length - 1].id : "manager") : v));
}

export type ToastKind = "ok" | "err" | "info";
export interface ToastAction {
  label: string;
  run: () => void;
}
export interface Toast {
  id: number;
  kind: ToastKind;
  text: string;
  action?: ToastAction;
}
export const toasts = writable<Toast[]>([]);

let toastSeq = 0;

export function dismissToast(id: number): void {
  toasts.update((list) => list.filter((t) => t.id !== id));
}

export function pushToast(kind: ToastKind, text: string, action?: ToastAction): number {
  const id = ++toastSeq;
  toasts.update((list) => [...list, { id, kind, text, action }]);
  // Give actionable toasts (e.g. Undo) a bit longer.
  setTimeout(() => dismissToast(id), action ? 7000 : 4500);
  return id;
}
