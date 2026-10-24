<script module lang="ts">
  let decoding = 0;
  const waiting: (() => void)[] = [];
  async function acquireDecoder() {
    if (decoding >= 2) await new Promise<void>(resolve => waiting.push(resolve));
    else decoding++;
    let released = false;
    return () => {
      if (released) return;
      released = true;
      const next = waiting.shift();
      if (next) next();
      else decoding--;
    };
  }
</script>

<script lang="ts">
  import { onMount } from "svelte";
  import { backgroundApi, mediaUrl, releaseVideo, type BackgroundAsset } from "../lib/background";

  let { asset }: { asset: BackgroundAsset } = $props();
  let host: HTMLDivElement;
  let image = $state("");
  let failed = $state(false);

  onMount(() => {
    let disposed = false;
    let started = false;
    let video: HTMLVideoElement | undefined;
    let release: (() => void) | undefined;
    let timeout: ReturnType<typeof setTimeout> | undefined;
    function stop() {
      clearTimeout(timeout);
      if (video) {
        video.onloadeddata = video.onseeked = video.onerror = null;
        releaseVideo(video);
        video = undefined;
      }
      release?.();
      release = undefined;
    }
    async function load() {
      if (started || disposed) return;
      started = true;
      if (asset.cover) {
        try {
          const path = await backgroundApi.thumbnail(asset.id);
          if (!disposed && path) { image = mediaUrl(path); return; }
        } catch { /* Missing or unsupported poster: extract a video frame. */ }
      }
      await frame();
    }
    async function frame() {
      if (disposed || video) return;
      image = "";
      if (asset.kind !== "video" || !asset.media) { failed = true; return; }
      try {
        const unlock = await acquireDecoder();
        if (disposed) { unlock(); return; }
        release = unlock;
        const resolved = await backgroundApi.resolve(asset.id);
        if (disposed) return;
        if (!resolved.media) throw new Error("missing media");
        const node = document.createElement("video");
        video = node;
        node.muted = node.defaultMuted = true;
        node.playsInline = true;
        node.preload = "auto";
        node.crossOrigin = "anonymous";
        const capture = () => {
          if (disposed || !node.videoWidth) return;
          try {
            const canvas = document.createElement("canvas");
            const scale = Math.min(320 / node.videoWidth, 180 / node.videoHeight);
            canvas.width = Math.max(1, Math.round(node.videoWidth * scale));
            canvas.height = Math.max(1, Math.round(node.videoHeight * scale));
            const context = canvas.getContext("2d");
            if (!context) throw new Error("canvas unavailable");
            context.drawImage(node, 0, 0, canvas.width, canvas.height);
            image = canvas.toDataURL("image/jpeg", 0.8);
          } catch { failed = true; }
          finally { stop(); }
        };
        node.onloadeddata = () => {
          // Skip a possible black intro, without playing any thumbnail video.
          const time = Number.isFinite(node.duration) ? Math.min(1, node.duration / 4) : 0;
          if (time > 0) node.currentTime = time;
          else capture();
        };
        node.onseeked = capture;
        node.onerror = () => { failed = true; stop(); };
        timeout = setTimeout(() => { failed = true; stop(); }, 12000);
        node.src = mediaUrl(resolved.media);
        node.load();
      } catch { if (!disposed) failed = true; stop(); }
    }
    // One-shot intersection loading also covers entries below the scrolling grid.
    const observer = new IntersectionObserver(entries => {
      if (entries.some(entry => entry.isIntersecting)) {
        observer.disconnect();
        void load();
      }
    });
    observer.observe(host);
    host.addEventListener("error", onImageError, true);
    function onImageError() { if (!failed) void frame(); }
    return () => {
      disposed = true;
      observer.disconnect();
      host.removeEventListener("error", onImageError, true);
      stop();
    };
  });
</script>

<div class="thumbnail" bind:this={host}>
  {#if image}<img src={image} alt="" loading="lazy" decoding="async" />
  {:else}<span>{failed ? "封面不可用" : "加载封面…"}</span>{/if}
</div>

<style>
  .thumbnail { width: 100%; height: 100%; display: grid; place-items: center; }
  img { width: 100%; height: 100%; min-height: 0; object-fit: cover; }
</style>
