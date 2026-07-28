import { writable } from "svelte/store";

// A tiny promise-based dialog service so the app never falls back to the native
// browser prompt()/confirm() (which look out of place). One dialog at a time.

export interface ConfirmOpts {
  title: string;
  message?: string;
  confirmLabel?: string;
  cancelLabel?: string;
  danger?: boolean;
}

export interface PromptOpts {
  title: string;
  message?: string;
  placeholder?: string;
  initial?: string;
  confirmLabel?: string;
}

export type DialogRequest =
  | { kind: "confirm"; opts: ConfirmOpts; resolve: (v: boolean) => void }
  | { kind: "prompt"; opts: PromptOpts; resolve: (v: string | null) => void };

export const activeDialog = writable<DialogRequest | null>(null);

/** Ask for confirmation. Resolves true (confirmed) / false (cancelled). */
export function confirmDialog(opts: ConfirmOpts): Promise<boolean> {
  return new Promise((resolve) => activeDialog.set({ kind: "confirm", opts, resolve }));
}

/** Ask for a line of text. Resolves the string, or null if cancelled. */
export function promptDialog(opts: PromptOpts): Promise<string | null> {
  return new Promise((resolve) => activeDialog.set({ kind: "prompt", opts, resolve }));
}
