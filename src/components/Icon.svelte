<script lang="ts">
  // 图标跟随「形状」设置：利落使用像素风方块图标（16×16 网格，填充），
  // 圆润 / 毛玻璃使用平滑的线条图标（24×24 网格，描边）。都跟随 currentColor。
  import { app } from "../lib/state.svelte";

  let { name, size = 18 }: { name: string; size?: number } = $props();

  const smoothMode = $derived((app.settings?.corner ?? "rounded") !== "sharp");

  const paths: Record<string, string> = {
    play: "M5 3h2v1h2v1h2v1h2v1h1v2h-1v1h-2v1h-2v1H7v1H5z",
    box: "M2 4h12v2H2zM3 6h10v7H3zm2 2v1h6V8z",
    download: "M7 2h2v6h2v1h-1v1H9v1H7v-1H6V9H5V8h2zM2 12h2v1h8v-1h2v3H2z",
    upload: "M7 11h2V5h2V4h-1V3H9V2H7v1H6v1H5v1h2zM2 12h2v1h8v-1h2v3H2z",
    cube: "M7 1h2v1h2v1h2v1h1v8h-1v1h-2v1H9v1H7v-1H5v-1H3v-1H2V4h1V3h2V2h2zM4 5v6h1v1h2V7H6V6H5V5zm8 0h-1v1h-1v1H9v5h2v-1h1z",
    user: "M5 2h6v1h1v5h-1v1H5V8H4V3h1zm1 2v2h1V4zm3 0v2h1V4zM3 10h10v1h1v3H2v-3h1z",
    gear: "M7 1h2v2h2V2h1v1h1v1h-1v2h2v2h-2v2h1v1h-1v1h-1v-1H9v2H7v-2H5v1H4v-1H3v-1h1V8H2V6h2V4H3V3h1V2h1v1h2zm0 5v1H6v2h1v1h2V9h1V7H9V6z",
    folder: "M1 3h5v1h1v1h8v9H1zm1 3v7h12V6z",
    trash: "M6 1h4v1h4v2H2V2h4zM3 5h10v10H3zm2 2v6h1V7zm3 0v6h1V7zm2 0v6h1V7z",
    refresh: "M6 2h5v1h1v1h1V2h1v5H9V6h3V5h-1V4H6v1H5v1H4v4h1v1h1v1h4v-1h1v-1h1V9h2v2h-1v1h-1v1h-1v1H5v-1H4v-1H3v-1H2V5h1V4h1V3h2z",
    search: "M5 1h4v1h1v1h1v4h-1v1h-1v1h1v1h1v1h1v1h1v1h1v2h-2v-1h-1v-1h-1v-1h-1v-1H9v-1H5V9H4V8H3V3h1V2h1zm0 2v1H4v3h1v1h3V7h1V4H8V3z",
    plus: "M7 2h2v5h5v2H9v5H7V9H2V7h5z",
    check: "M12 3h2v2h-1v1h-1v1h-1v1h-1v1H9v1H8v1H7v1H6v-1H5v-1H4V9H3V8H2V6h2v1h1v1h1v1h1V8h1V7h1V6h1V5h1V4h1z",
    x: "M3 3h2v1h1v1h1v1h2V5h1V4h1V3h2v2h-1v1h-1v1H9v2h1v1h1v1h1v2h-2v-1H9v-1H7v1H6v1H5v1H3v-2h1v-1h1v-1h1V9h1V7H6V6H5V5H4V4H3z",
    terminal: "M1 2h14v12H1zm1 2v9h12V4zm2 1h1v1h1v1h1v1H6v1H5v1H4V9h1V8h1V7H5V6H4zm4 4h4v1H8z",
    stop: "M4 4h8v8H4z",
    java: "M7 1h1v1h1v2H8v1H7v1h1v1H7V6H6V4h1V3h1V2H7zM3 8h9v1h1v1h1v1h-2v1h-1v1H5v-1H4v-1H3zm1 6h8v1H4z",
    warn: "M7 1h2v2h1v2h1v2h1v2h1v2h1v2h1v2H1v-2h1v-2h1V9h1V7h1V5h1V3h1zm0 4v5h2V5zm0 6v2h2v-2z",
    info: "M7 2h2v2H7zM6 6h3v6h1v2H6v-2h1V8H6z",
    star: "M7 1h2v4h5v2h-1v1h-1v1h-1v1h1v4h-1v1h-1v-1H9v-1H7v1H6v1H5v-1H4v-4h1V9H4V8H3V7H2V5h5z",
    chevron: "M6 3h2v1h1v1h1v1h1v1h1v2h-1v1h-1v1H9v1H8v1H6v-2h1v-1h1V9h1V7H8V6H7V5H6z",
    key: "M3 4h5v1h1v2h6v2h-1v2h-2V9h-1v1H9v1H8v1H3v-1H2V5h1zm1 2v4h3V6z",
    link: "M3 5h5v2H4v2h4v2H3v-1H2V6h1zm5 0h5v1h1v4h-1v1H8V9h4V7H8zM5 7h6v2H5z",
  };

  // 平滑图标：描边路径；fill 为 true 的图标同时填充
  const smooth: Record<string, { d: string; fill?: boolean }> = {
    play: { d: "M7.5 5.2v13.6a.8.8 0 0 0 1.2.7l10.6-6.8a.8.8 0 0 0 0-1.4L8.7 4.5a.8.8 0 0 0-1.2.7z", fill: true },
    box: { d: "M4 4.5h16a1 1 0 0 1 1 1V8a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V5.5a1 1 0 0 1 1-1z M5 9v9.5A1.5 1.5 0 0 0 6.5 20h11a1.5 1.5 0 0 0 1.5-1.5V9 M10 13h4" },
    download: { d: "M12 3.5v11 M7.5 10.5l4.5 4.5 4.5-4.5 M4 16.5v2A1.5 1.5 0 0 0 5.5 20h13a1.5 1.5 0 0 0 1.5-1.5v-2" },
    upload: { d: "M12 15V4 M7.5 8.5L12 4l4.5 4.5 M4 16.5v2A1.5 1.5 0 0 0 5.5 20h13a1.5 1.5 0 0 0 1.5-1.5v-2" },
    cube: { d: "M12 2.8l8.2 4.6v9.2L12 21.2l-8.2-4.6V7.4z M3.8 7.4L12 12l8.2-4.6 M12 12v9.2" },
    user: { d: "M16 8a4 4 0 1 1-8 0a4 4 0 1 1 8 0z M4.5 20.5c.6-3.6 3.6-6 7.5-6s6.9 2.4 7.5 6" },
    gear: {
      d: "M18.99 10.26L21.49 10.57L21.49 13.43L18.99 13.74L18.17 15.71L19.72 17.7L17.7 19.72L15.71 18.17L13.74 18.99L13.43 21.49L10.57 21.49L10.26 18.99L8.29 18.17L6.3 19.72L4.28 17.7L5.83 15.71L5.01 13.74L2.51 13.43L2.51 10.57L5.01 10.26L5.83 8.29L4.28 6.3L6.3 4.28L8.29 5.83L10.26 5.01L10.57 2.51L13.43 2.51L13.74 5.01L15.71 5.83L17.7 4.28L19.72 6.3L18.17 8.29z M15 12a3 3 0 1 1-6 0a3 3 0 1 1 6 0z",
    },
    folder: { d: "M3 6.5A1.5 1.5 0 0 1 4.5 5h4.3l2 2.5h8.7A1.5 1.5 0 0 1 21 9v9.5a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 18.5z" },
    trash: { d: "M4 6.5h16 M9 6.5V4.5a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2 M6 6.5l.9 12.6A1.5 1.5 0 0 0 8.4 20.5h7.2a1.5 1.5 0 0 0 1.5-1.4L18 6.5 M10 10.5v6 M14 10.5v6" },
    refresh: { d: "M19.5 12a7.5 7.5 0 1 1-2.2-5.3 M19.5 4v4.5H15" },
    search: { d: "M17 10.5a6.5 6.5 0 1 1-13 0a6.5 6.5 0 1 1 13 0z M15.3 15.3L20 20" },
    plus: { d: "M12 5v14 M5 12h14" },
    check: { d: "M5 12.5l4.5 4.5L19 7.5" },
    x: { d: "M6.5 6.5l11 11 M17.5 6.5l-11 11" },
    terminal: { d: "M4.5 4h15A1.5 1.5 0 0 1 21 5.5v13a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 18.5v-13A1.5 1.5 0 0 1 4.5 4z M7 9l3 3-3 3 M12.5 15H17" },
    stop: { d: "M7.5 6h9A1.5 1.5 0 0 1 18 7.5v9a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 6 16.5v-9A1.5 1.5 0 0 1 7.5 6z", fill: true },
    java: { d: "M5 10h11v4.5a4.5 4.5 0 0 1-4.5 4.5h-2A4.5 4.5 0 0 1 5 14.5z M16 11h1.5a2.5 2.5 0 0 1 0 5H16 M9 3.5c-1 1.2 1 2.3 0 3.5 M12.5 3.5c-1 1.2 1 2.3 0 3.5 M4 21h13" },
    warn: { d: "M10.7 4.2a1.5 1.5 0 0 1 2.6 0l7.6 13.6a1.5 1.5 0 0 1-1.3 2.2H4.4a1.5 1.5 0 0 1-1.3-2.2z M12 9.5v4.5 M12 17h.01" },
    info: { d: "M21 12a9 9 0 1 1-18 0a9 9 0 1 1 18 0z M12 11v5.5 M12 7.8h.01" },
    star: { d: "M12 3.1L14.47 9.2L21.04 9.66L15.99 13.9L17.58 20.29L12 16.8L6.42 20.29L8.01 13.9L2.96 9.66L9.53 9.2z" },
    chevron: { d: "M9 5.5l6.5 6.5L9 18.5" },
    key: { d: "M12 15a4 4 0 1 1-8 0a4 4 0 1 1 8 0z M10.8 12.2L20 3 M16.5 6.5l3 3 M14 9l2 2" },
    link: { d: "M10 14a4 4 0 0 1 0-5.66l2.5-2.5a4 4 0 0 1 5.66 5.66L16.5 13 M14 10a4 4 0 0 1 0 5.66l-2.5 2.5a4 4 0 0 1-5.66-5.66L7.5 11" },
  };
  const icon = $derived(smooth[name] ?? smooth.box);
</script>

{#if smoothMode}
  <svg
    width={size}
    height={size}
    viewBox="0 0 24 24"
    fill={icon.fill ? "currentColor" : "none"}
    stroke="currentColor"
    stroke-width="2"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
  >
    <path d={icon.d} />
  </svg>
{:else}
  <svg
    width={size}
    height={size}
    viewBox="0 0 16 16"
    fill="currentColor"
    shape-rendering="crispEdges"
    aria-hidden="true"
  >
    <path d={paths[name] ?? paths.box} fill-rule="evenodd" />
  </svg>
{/if}
