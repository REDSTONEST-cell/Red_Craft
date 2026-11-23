<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { backgroundApi, shouldPlay, yieldsToGame, controlVideo, releaseVideo, observeVisibility, webUrl, type ResolvedBackground } from "../lib/background";
  import { background, loadBackground } from "../lib/background-state.svelte";
  import { app, showError, toast } from "../lib/state.svelte";

  const appearance = $derived(background.preview ?? background.saved);
  const selectedId = $derived(appearance.selection);
  let media = $state<ResolvedBackground | null>(null);
  let videoFailed = $state(false);
  let visible = $state(false);
  let reduced = $state(window.matchMedia("(prefers-reduced-motion: reduce)").matches);
  let player = $state<HTMLVideoElement | null>(null);
  let generation = 0;
  const mediaUrl = (path: string | null) => path ? convertFileSrc(path) : "";
  const dynamic = $derived(appearance.motion === "always" || (appearance.motion === "auto" && !reduced));
  let focused = $state(document.hasFocus());
  const gameRunning = $derived(Object.values(app.games).some(g => g.running));
  const playing = $derived(shouldPlay(visible && !yieldsToGame(gameRunning, focused), appearance.motion, reduced));
  // 网页壁纸只在需要播放时运行（窗口隐藏、游戏运行或静态模式下卸载 iframe，改为显示封面）。
  const webLive = $derived(!!media && media.kind === "web" && !!media.entry && playing);
  const hasMedia = $derived(!!media && (((media.kind === "image" || media.kind === "video") && dynamic) || webLive || !!media.cover));

  onMount(() => {
    const mq = window.matchMedia("(prefers-reduced-motion: reduce)");
    const change = () => { reduced = mq.matches; };
    const observer = observeVisibility({
      query: () => backgroundApi.visible(),
      subscribe: receive => listen<boolean>("background-window-visible", e => receive(e.payload)),
      hidden: () => document.hidden,
      change: value => { visible = value; },
    });
    const focus = () => { focused = true; };
    const blur = () => { focused = false; };
    window.addEventListener("focus", focus);
    window.addEventListener("blur", blur);
    mq.addEventListener("change", change);
    document.addEventListener("visibilitychange", observer.refresh);
    // Native hide/minimize events and explicit launcher hide/show are primary.
    // The initial native query also handles a window already hidden on mount.
    void loadBackground().catch(e => showError(e, "读取背景失败"));
    return () => {
      window.removeEventListener("focus", focus);
      window.removeEventListener("blur", blur);
      mq.removeEventListener("change", change);
      document.removeEventListener("visibilitychange", observer.refresh);
      observer.dispose();
    };
  });

  $effect(() => {
    const id = selectedId;
    // Explicit preview can retry a restored file without changing its ID.
    void background.retry;
    const token = ++generation;
    media = null;
    videoFailed = false;
    background.error = "";
    background.ready = !id;
    if (id) void backgroundApi.resolve(id).then(value => {
      if (token === generation) { media = value; background.ready = (value.kind === "video" || value.kind === "web") && !dynamic; }
    }).catch(e => {
      if (token === generation) fail(e);
    });
    return () => { if (token === generation) generation++; };
  });

  function fail(e: unknown) {
    media = null;
    background.ready = false;
    background.error = typeof e === "object" && e && "message" in e ? String(e.message) : String(e);
    showError(e, "背景不可用，已回退主题底色");
  }

  function manage(node: HTMLVideoElement) {
    const token = generation;
    const failure = () => {
      if (token !== generation) return;
      // 系统无法解码这个视频（例如 HEVC）：有预览图时改为显示预览图，而不是整个背景失效。
      if (media?.cover) {
        videoFailed = true;
        toast("info", "视频无法解码，已改用预览图", "WebView2 不支持这个视频的编码（例如 HEVC），建议使用 H.264 MP4。");
        return;
      }
      fail("视频无法解码。建议使用 H.264 MP4，或重新导入素材。");
    };
    const loaded = () => { if (token === generation) background.ready = true; };
    background.ready = false;
    node.addEventListener("error", failure);
    node.addEventListener("loadeddata", loaded);
    return { destroy() {
      node.removeEventListener("error", failure);
      node.removeEventListener("loadeddata", loaded);
      releaseVideo(node);
    } };
  }

  function observeFrame(node: HTMLIFrameElement) {
    const token = generation;
    const loaded = () => { if (token === generation) background.ready = true; };
    node.addEventListener("load", loaded);
    return { destroy() { node.removeEventListener("load", loaded); node.src = "about:blank"; } };
  }

  function observeImage(node: HTMLImageElement) {
    const token = generation;
    const loaded = () => { if (token === generation) background.ready = true; };
    const failed = () => { if (token === generation) fail("图片无法显示，请重新导入。"); };
    node.addEventListener("load", loaded);
    node.addEventListener("error", failed);
    if (node.complete && node.naturalWidth) loaded();
    return { destroy() { node.removeEventListener("load", loaded); node.removeEventListener("error", failed); } };
  }

  $effect(() => {
    const node = player;
    const token = generation;
    if (!node) return;
    // Svelte 5 的布尔 muted 属性不一定反射到原生媒体属性。
    // 必须先写 property，再请求播放；不能只依赖模板 muted。
    node.muted = true;
    return controlVideo(node, playing, e => { if (token === generation && player === node) fail(e); });
  });

  $effect(() => {
    document.documentElement.classList.toggle("has-background", hasMedia);
    return () => document.documentElement.classList.remove("has-background");
  });
</script>

<div class="background-media" aria-hidden="true" style:--fit={appearance.fit}
  style:--brightness={appearance.brightness} style:--blur="{appearance.blurPx}px"
  style:--media-opacity={appearance.backgroundOpacity}>
  {#if media}
    {#key media.id}
      {#if media.kind === "image" && (dynamic ? media.media : media.cover)}
        <img class="media" use:observeImage src={mediaUrl(dynamic ? media.media : media.cover)} alt="" />
      {:else if webLive}
        <!-- 不同源 + 沙盒：页面无法访问启动器，也拿不到 IPC；不允许弹窗、表单与顶层跳转。 -->
        <iframe class="media web" use:observeFrame src={webUrl(media.id, media.entry!, media.host)} title="" tabindex="-1"
          sandbox="allow-scripts allow-same-origin" allow="camera 'none'; microphone 'none'; geolocation 'none'"
          referrerpolicy="no-referrer"></iframe>
      {:else if media.kind === "video" && media.media && dynamic && !videoFailed}
        <video class="media" bind:this={player} use:manage src={mediaUrl(media.media)} autoplay muted loop playsinline preload="metadata" tabindex="-1"></video>
      {:else if media.cover}
        <img class="media" use:observeImage src={mediaUrl(media.cover)} alt="" />
      {/if}
    {/key}
  {/if}
</div>
{#if hasMedia}<div class="background-mask" aria-hidden="true" style:--mask-opacity={appearance.overlayOpacity}></div>{/if}

<style>
  .background-media { position: fixed; inset: 0; z-index: 0; pointer-events: none; overflow: hidden; }
  .media { position: absolute; inset: calc(-1 * var(--blur)); width: calc(100% + 2 * var(--blur)); height: calc(100% + 2 * var(--blur)); object-fit: var(--fit); opacity: var(--media-opacity); filter: brightness(var(--brightness)) blur(var(--blur)); }
  .web { border: 0; object-fit: initial; background: transparent; }
  .background-mask { position: fixed; inset: 0; z-index: 1; pointer-events: none; background: rgba(0, 0, 0, var(--mask-opacity)); }
</style>
