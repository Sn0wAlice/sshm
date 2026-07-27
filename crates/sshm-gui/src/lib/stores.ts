import { writable } from "svelte/store";
import type { Host } from "./bindings";

export type Tab = "hosts" | "tunnels" | "kluster" | "identities" | "settings";

export const activeTab = writable<Tab>("hosts");

export const hosts = writable<Host[]>([]);
export const folders = writable<string[]>([]);
export const selectedHostName = writable<string | null>(null);

export type ToastKind = "ok" | "err";
export interface Toast {
  id: number;
  kind: ToastKind;
  text: string;
}

export const toasts = writable<Toast[]>([]);

let nextId = 0;

export function pushToast(kind: ToastKind, text: string): void {
  const id = ++nextId;
  toasts.update((list) => [...list, { id, kind, text }]);
  setTimeout(() => {
    toasts.update((list) => list.filter((t) => t.id !== id));
  }, 4500);
}
