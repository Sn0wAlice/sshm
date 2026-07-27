import { writable } from "svelte/store";
import type { Host } from "./bindings";

// Left-rail sections (the "manager" surface).
export type Section = "hosts" | "portforward" | "kluster" | "keychain" | "settings";
export const activeSection = writable<Section>("hosts");

// Top strip: the host manager plus one tab per open embedded terminal.
export interface Session {
  id: string; // frontend id (also drives the Terminal component)
  host: string;
  title: string;
}
export const sessions = writable<Session[]>([]);
/** "manager" shows the section views; a session id shows that terminal. */
export const activeView = writable<"manager" | string>("manager");

export const hosts = writable<Host[]>([]);
export const folders = writable<string[]>([]);
export const selectedHostName = writable<string | null>(null);

let sessionSeq = 0;
export function openSession(host: string): string {
  const id = `t${++sessionSeq}`;
  sessions.update((s) => [...s, { id, host, title: host }]);
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

export type ToastKind = "ok" | "err";
export interface Toast {
  id: number;
  kind: ToastKind;
  text: string;
}
export const toasts = writable<Toast[]>([]);

let toastSeq = 0;
export function pushToast(kind: ToastKind, text: string): void {
  const id = ++toastSeq;
  toasts.update((list) => [...list, { id, kind, text }]);
  setTimeout(() => {
    toasts.update((list) => list.filter((t) => t.id !== id));
  }, 4500);
}
