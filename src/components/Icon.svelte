<script lang="ts">
  // 像素风图标：16×16 网格，用 path 绘制，跟随 currentColor。
  let { name, size = 18 }: { name: string; size?: number } = $props();

  const paths: Record<string, string> = {
    play: "M5 3h2v1h2v1h2v1h2v1h1v2h-1v1h-2v1h-2v1H7v1H5z",
    box: "M2 4h12v2H2zM3 6h10v7H3zm2 2v1h6V8z",
    download: "M7 2h2v6h2v1h-1v1H9v1H7v-1H6V9H5V8h2zM2 12h2v1h8v-1h2v3H2z",
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
</script>

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
