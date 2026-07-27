import type { Host } from "./bindings";

// A curated, Termius-ish palette of icon backgrounds.
const PALETTE = [
  "#e8590c", // orange (ubuntu-ish)
  "#d6336c", // pink/red (debian-ish)
  "#f59f00", // amber
  "#1c7ed6", // blue
  "#7048e8", // violet
  "#0ca678", // teal
  "#e03131", // red
  "#4263eb", // indigo
  "#2b8a3e", // green
];

function hash(s: string): number {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}

export interface HostIcon {
  bg: string;
  label: string;
}

/** Deterministic colored square + monogram for a host (a stand-in for an OS
 *  logo). Keyword hints nudge the color for common roles. */
export function hostIcon(h: Host): HostIcon {
  const hay = `${h.name} ${(h.tags ?? []).join(" ")} ${h.notes ?? ""}`.toLowerCase();
  let bg: string;
  if (/\b(db|database|postgres|mysql|redis|mongo)\b/.test(hay)) bg = "#4263eb";
  else if (/\b(aws|ec2|amazon)\b/.test(hay)) bg = "#e8590c";
  else if (/\b(prod|production)\b/.test(hay)) bg = "#e03131";
  else if (/\b(staging|stage|qa)\b/.test(hay)) bg = "#f59f00";
  else if (/\b(dev|development)\b/.test(hay)) bg = "#0ca678";
  else bg = PALETTE[hash(h.name) % PALETTE.length];
  const label = (h.name.replace(/[^a-z0-9]/gi, "")[0] ?? "?").toUpperCase();
  return { bg, label };
}
