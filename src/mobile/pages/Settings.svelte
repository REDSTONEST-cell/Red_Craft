<script lang="ts">
  // 手机版设置：只保留手机上用得到的选项，按分组排成卡片列表（类似系统设置）
  import { api, type DownloadSource, type MemoryStatus, type Settings } from "../../lib/api";
  import { app, saveSettings } from "../../lib/state.svelte";
  import Segmented from "../../components/Segmented.svelte";
  import Toggle from "../../components/Toggle.svelte";

  const s = $derived(app.settings!);
  function save(patch: Partial<Settings>) {
    return saveSettings(patch);
  }

  const accents = ["#E8590C", "#C2410C", "#B45309", "#2F7D32", "#1D4ED8", "#7C3AED", "#BE185D", "#1B1A17"];
  const sources: { value: DownloadSource; label: string }[] = [
    { value: "bmclapi", label: "镜像优先" },
    { value: "auto", label: "自动" },
    { value: "official", label: "官方优先" },
  ];
  const SPEED_STEPS = [256, 512, 1024, 2048, 5120, 10240, 0];
  const speedLabel = (kb: number) => (!kb ? "不限速" : kb >= 1024 ? `${+(kb / 1024).toFixed(1)} MB/s` : `${kb} KB/s`);
  const nearestStep = (kb: number) =>
    !kb ? SPEED_STEPS.length - 1 : SPEED_STEPS.reduce((best, v, i) => (v && Math.abs(v - kb) < Math.abs(SPEED_STEPS[best] - kb) ? i : best), 0);
  // 读不到手机内存时（totalMb 为 0）按 4GB 显示
  const total = $derived(app.info?.memory.totalMb || 4096);
  const maxMem = $derived(Math.max(1024, Math.floor((total * 0.75) / 256) * 256));

  // 自动分配时显示启动当前版本会分配的内存（与启动时的计算相同）
  let mem = $state<MemoryStatus | null>(null);
  $effect(() => {
    void [s?.memoryAuto, s?.maxMemoryMb, app.settings?.selectedInstance];
    api
      .memoryStatus()
      .then((m) => (mem = m))
      .catch(() => (mem = null));
  });

  const memoryHint = $derived.by(() => {
    const base = "根据手机内存决定（与 FCL 相同）";
    if (!s?.memoryAuto || !mem?.gameMb) return base;
    const target = app.settings?.selectedInstance ? `「${app.settings.selectedInstance}」` : "游戏";
    return `${base}；启动${target}时分配 ${(mem.gameMb / 1024).toFixed(1)} GB`;
  });

  // 手机版的版本号由 Android 端提供
  const host = (window as unknown as { RclGame?: { appVersion?: () => string } }).RclGame;
  const version = host?.appVersion?.() ?? "";
</script>

{#if s}
  <div class="wrap">
    <h1>设置</h1>

    <section>
      <h2>外观</h2>
      <div class="item">
        <span class="label">主题</span>
        <Segmented
          value={s.theme}
          options={[
            { value: "light", label: "浅色" },
            { value: "dark", label: "深色" },
            { value: "system", label: "跟随系统" },
          ]}
          onchange={(v) => save({ theme: v })}
        />
      </div>
      <div class="item">
        <span class="label">主题色</span>
        <div class="swatches">
          {#each accents as c}
            <button
              class="swatch"
              class:active={s.accent.toLowerCase() === c.toLowerCase()}
              style:background={c}
              aria-label={c}
              onclick={() => save({ accent: c })}
            ></button>
          {/each}
        </div>
      </div>
    </section>

    <section>
      <h2>游戏</h2>
      <div class="item">
        <span class="label">自动分配内存<small>{memoryHint}</small></span>
        <Toggle checked={s.memoryAuto} onchange={(v) => save({ memoryAuto: v })} />
      </div>
      {#if !s.memoryAuto}
        <div class="item">
          <span class="label">最大内存<small>手机共 {(total / 1024).toFixed(1)} GB</small></span>
          <div class="range">
            <input
              type="range"
              min="512"
              max={maxMem}
              step="256"
              value={Math.min(s.maxMemoryMb, maxMem)}
              onchange={(e) => save({ maxMemoryMb: +e.currentTarget.value })}
            />
            <span class="val">{(Math.min(s.maxMemoryMb, maxMem) / 1024).toFixed(2)} GB</span>
          </div>
        </div>
      {/if}
      <div class="item">
        <span class="label">游戏语言设为简体中文<small>第一次启动新版本时生效</small></span>
        <Toggle checked={s.defaultChinese} onchange={(v) => save({ defaultChinese: v })} />
      </div>
    </section>

    <section>
      <h2>下载</h2>
      <div class="item">
        <span class="label">游戏文件<small>游戏本体、依赖库与加载器；镜像为 BMCLAPI</small></span>
        <Segmented value={s.downloadSource} options={sources} onchange={(v) => save({ downloadSource: v })} />
      </div>
      <div class="item">
        <span class="label">版本列表</span>
        <Segmented value={s.versionSource} options={sources} onchange={(v) => save({ versionSource: v })} />
      </div>
      <div class="item">
        <span class="label">同时下载<small>网络不稳定时可以调低</small></span>
        <div class="range">
          <input type="range" min="4" max="64" step="4" value={s.concurrency} onchange={(e) => save({ concurrency: +e.currentTarget.value })} />
          <span class="val">{s.concurrency}</span>
        </div>
      </div>
      <div class="item">
        <span class="label">速度限制</span>
        <div class="range">
          <input
            type="range"
            min="0"
            max={SPEED_STEPS.length - 1}
            step="1"
            value={nearestStep(s.speedLimitKb)}
            onchange={(e) => save({ speedLimitKb: SPEED_STEPS[+e.currentTarget.value] })}
          />
          <span class="val wide">{speedLabel(s.speedLimitKb)}</span>
        </div>
      </div>
    </section>

    <section>
      <h2>关于</h2>
      <div class="item">
        <span class="label">Red Craft Launcher 手机版<small>{version ? `版本 ${version}` : "手机版"} · 支持 1.17 ~ 26.x</small></span>
      </div>
      <div class="item">
        <span class="label">开源许可<small>GPL-3.0 · 游戏运行环境基于 Fold Craft Launcher</small></span>
      </div>
    </section>
  </div>
{/if}

<style>
  .wrap {
    max-width: 720px;
  }
  h1 {
    font-size: 20px;
    font-weight: 800;
    margin-bottom: 10px;
  }
  section {
    margin-bottom: 12px;
    border-radius: 14px;
    background: var(--card);
    border: 1px solid var(--line);
    overflow: hidden;
  }
  h2 {
    padding: 10px 14px 4px;
    font-size: 12px;
    font-weight: 700;
    color: var(--accent);
  }
  .item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    min-height: 48px;
    padding: 6px 14px;
  }
  .item + .item {
    border-top: 1px solid var(--line);
  }
  .label {
    display: flex;
    flex-direction: column;
    font-size: 13.5px;
    font-weight: 600;
    min-width: 0;
  }
  .label small {
    font-size: 11px;
    font-weight: 400;
    color: var(--muted);
  }
  .swatches {
    display: flex;
    gap: 7px;
    flex-wrap: wrap;
    justify-content: flex-end;
  }
  .swatch {
    width: 24px;
    height: 24px;
    border-radius: 50%;
    border: 2px solid transparent;
    box-shadow: inset 0 0 0 2px var(--card);
  }
  .swatch.active {
    border-color: var(--ink);
  }
  .range {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .range input {
    width: 170px;
    accent-color: var(--accent);
  }
  .val {
    min-width: 54px;
    text-align: right;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }
  .val.wide {
    min-width: 64px;
  }
</style>
