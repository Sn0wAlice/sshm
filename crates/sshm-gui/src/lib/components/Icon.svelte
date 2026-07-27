<script lang="ts">
  // Minimal inline-SVG icon set (stroke-based, currentColor).
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
    | "menu";
  export let size = 18;

  const paths: Record<string, string> = {
    hosts:
      "M3 5h18v6H3zM3 13h18v6H3zM7 8h.01M7 16h.01",
    portforward: "M4 17l6-6-6-6M12 19h8",
    kluster:
      "M21 16V8l-9-5-9 5v8l9 5 9-5zM3.3 7.3L12 12l8.7-4.7M12 22V12",
    keychain:
      "M7 14a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM11 10h10M17 10v4M21 10v3",
    settings:
      "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-2.9 1.2V21a2 2 0 1 1-4 0v-.1A1.7 1.7 0 0 0 6 19.4l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0-1.2-2.9H2a2 2 0 1 1 0-4h.1A1.7 1.7 0 0 0 3.4 6l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1A1.7 1.7 0 0 0 9 3.4V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 2.9 1.2l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.2 2.9",
    terminal: "M4 5l7 7-7 7M12 19h8",
    plus: "M12 5v14M5 12h14",
    close: "M6 6l12 12M18 6L6 18",
    connect: "M5 12h14M13 6l6 6-6 6",
    search: "M11 11m-7 0a7 7 0 1 0 14 0 7 7 0 1 0-14 0M21 21l-4.3-4.3",
    folder: "M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z",
    star: "M12 3l2.9 5.9 6.5.9-4.7 4.6 1.1 6.5L12 17.8 6.2 21l1.1-6.5L2.6 9.8l6.5-.9z",
    menu: "M4 6h16M4 12h16M4 18h16",
  };
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="1.8"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  <path d={paths[name]} />
</svg>
