// Thin layer over the generated tauri-specta bindings.
//
// Re-exports every command/event/type, plus `unwrap` to turn a Rust
// `Result<T, String>` into a value-or-throw, and `run` to funnel errors into a
// toast so component code stays terse.

import type { Result } from "./bindings";
import { commands } from "./bindings";
import { addSession, pushToast } from "./stores";

export * from "./bindings";
export { commands, events } from "./bindings";

/** Turn a `Result<T, string>` into `T`, throwing the error string. */
export function unwrap<T>(r: Result<T, string>): T {
  if (r.status === "ok") return r.data;
  throw new Error(r.error);
}

/**
 * Await a command that returns `Result`, unwrap it, and on error surface a
 * toast and rethrow. Returns the unwrapped value on success.
 */
export async function run<T>(p: Promise<Result<T, string>>): Promise<T> {
  const value = unwrap(await p);
  return value;
}

/** Like `run` but swallows the error (after toasting) and returns undefined. */
export async function tryRun<T>(
  p: Promise<Result<T, string>>,
  onOk?: string,
): Promise<T | undefined> {
  try {
    const v = unwrap(await p);
    if (onOk) pushToast("ok", onOk);
    return v;
  } catch (e) {
    pushToast("err", e instanceof Error ? e.message : String(e));
    return undefined;
  }
}

/**
 * Await a command that spawns a backend PTY session (returns its id), then open
 * a terminal tab for it. Toasts on error. Used by ssh / local / kluster shells.
 */
export async function openBackendSession(
  p: Promise<Result<string, string>>,
  title: string,
): Promise<void> {
  const r = await p;
  if (r.status === "error") {
    pushToast("err", r.error);
    return;
  }
  addSession(r.data, title);
}

/** Open an embedded ssh session to a saved host. */
export const openHostSession = (host: string): Promise<void> =>
  openBackendSession(commands.termOpen(host), host);

/** Open a local shell in a new tab. */
export const openLocalSession = (): Promise<void> =>
  openBackendSession(commands.termOpenLocal(), "local");
