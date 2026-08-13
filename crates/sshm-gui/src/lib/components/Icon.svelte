<script lang="ts">
  // Thin wrapper over Lucide so the rest of the app keeps the `<Icon name … />`
  // API while getting a cohesive, professional icon set.
  import {
    Server,
    Waypoints,
    Boxes,
    KeyRound,
    Settings,
    SquareTerminal,
    Plus,
    X,
    ChevronRight,
    Search,
    Folder,
    Star,
    Menu,
    ShieldCheck,
    type Icon as IconType,
  } from "lucide-svelte";

  export let name:
    | "hosts"
    | "portforward"
    | "kluster"
    | "keychain"
    | "settings"
    | "terminal"
    | "plus"
    | "close"
    | "connect"
    | "search"
    | "folder"
    | "star"
    | "shield"
    | "menu";
  export let size = 18;

  const map: Record<string, typeof IconType> = {
    hosts: Server,
    portforward: Waypoints,
    kluster: Boxes,
    keychain: KeyRound,
    settings: Settings,
    terminal: SquareTerminal,
    plus: Plus,
    close: X,
    connect: ChevronRight,
    search: Search,
    folder: Folder,
    star: Star,
    shield: ShieldCheck,
    menu: Menu,
  };
</script>

<svelte:component this={map[name]} {size} strokeWidth={1.75} />
