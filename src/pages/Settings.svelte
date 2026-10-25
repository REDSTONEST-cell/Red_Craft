<script lang="ts">
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { api, type JavaInstall, type Settings } from "../lib/api";
  import { app, refreshInstances, saveSettings, showError, toast } from "../lib/state.svelte";
  import Segmented from "../components/Segmented.svelte";
  import Toggle from "../components/Toggle.svelte";
  import Icon from "../components/Icon.svelte";
  import BackgroundPicker from "../components/BackgroundPicker.svelte";
  import { page } from "../lib/motion";

  // 本地副本：文本框编辑时不立即写盘，失焦后保存
  let s = $state<Settings>(structuredClone($state.snapshot(app.settings!)));
  let javas = $state<JavaInstall[] | null>(null);
  let scanning = $state(false);
  let advanced = $state(false);

  function save(patch: Partial<Settings>) {
    Object.assign(s, patch);
    return saveSettings(patch);
  }

  async function scan(refresh: boolean) {
    scanning = true;
    try {
      javas = await api.scanJava(refresh);
    } catch (e) {
      showError(e, "扫描 Java 失败");
    } finally {
      scanning = false;
    }
  }

  $effect(() => {
    scan(false);
  });

  async function pickGameDir() {
    const dir = await openDialog({ directory: true, title: "选择游戏目录（.minecraft）" });
    if (typeof dir === "string") {
      await save({ gameDir: dir });
      app.info = await api.appInfo();
      await refreshInstances();
      toast("ok", "游戏目录已更改", dir);
    }
  }

  async function resetGameDir() {
    await save({ gameDir: null });
    app.info = await api.appInfo();
    await refreshInstances();
  }

  async function pickJava() {
    const file = await openDialog({ title: "选择 java 可执行文件", multiple: false, directory: false });
    if (typeof file !== "string") return;
    try {
      const info = await api.probeJava(file);
      await save({ javaPath: info.path });
      toast("ok", `已选择 Java ${info.version}`, info.path);
      await scan(true);
    } catch (e) {
      showError(e, "无效的 Java");
    }
  }

  const total = $derived(app.info?.memory.totalMb ?? 8192);
  const accents = ["#E8590C", "#C2410C", "#B45309", "#2F7D32", "#1D4ED8", "#7C3AED", "#BE185D", "#1B1A17"];
</script>

<div class="page-head">
  <div>
    <div class="kicker">SETTINGS</div>
    <h1>设置</h1>
    <div class="sub">修改后自动保存</div>
  </div>
</div>

<div class="sections">
  <section>
    <h3>游戏目录</h3>
    <div class="row">
      <div class="desc">
        <strong>.minecraft 位置</strong>
        <span class="mono path">{app.info?.gameDir}</span>
        {#if app.info?.portable && !s.gameDir}<span class="help">便携模式：使用启动器所在目录下的 .minecraft</span>{/if}
      </div>
      <div class="ctl">
        <button class="btn sm" onclick={pickGameDir}>更改</button>
        {#if s.gameDir}<button class="btn sm ghost" onclick={resetGameDir}>恢复默认</button>{/if}
        <button class="btn sm icon" title="打开" onclick={() => api.openGameDir()}><Icon name="folder" size={14} /></button>
      </div>
    </div>
    <div class="row">
      <div class="desc">
        <strong>版本隔离</strong>
        <span class="help">开启隔离的实例使用 versions/实例名 作为游戏目录，存档和 Mod 互不干扰。每个实例也可单独设置。</span>
      </div>
      <select class="input" value={s.isolation} onchange={(e) => save({ isolation: e.currentTarget.value as Settings["isolation"] })}>
        <option value="off">关闭</option>
        <option value="modded">隔离可安装 Mod 的版本</option>
        <option value="nonRelease">隔离非正式版</option>
        <option value="moddedAndNonRelease">隔离可安装 Mod 的版本与非正式版</option>
        <option value="all">隔离所有版本</option>
      </select>
    </div>
  </section>

  <section>
    <h3>下载</h3>
    <div class="row">
      <div class="desc">
        <strong>下载源</strong>
        <span class="help">「自动」优先使用官方源，失败时切换到 BMCLAPI 镜像；国内网络较慢时可直接选择 BMCLAPI。</span>
      </div>
      <Segmented
        value={s.downloadSource}
        options={[
          { value: "auto", label: "自动" },
          { value: "official", label: "官方" },
          { value: "bmclapi", label: "BMCLAPI" },
        ]}
        onchange={(v) => save({ downloadSource: v })}
      />
    </div>
    <div class="row">
      <div class="desc">
        <strong>并发数</strong>
        <span class="help">同时下载的文件数量。网络不稳定时可以调低。</span>
      </div>
      <div class="ctl range">
        <input
          type="range"
          min="4"
          max="128"
          step="4"
          bind:value={s.concurrency}
          onchange={() => save({ concurrency: s.concurrency })}
        />
        <span class="mono val">{s.concurrency}</span>
      </div>
    </div>
  </section>

  <section>
    <h3>Java</h3>
    <div class="row">
      <div class="desc">
        <strong>Java 路径</strong>
        <span class="help">
          {#if s.javaPath}已手动指定：<span class="mono">{s.javaPath}</span>{:else}自动：根据游戏版本选择合适的 Java{/if}
        </span>
      </div>
      <div class="ctl">
        <button class="btn sm" onclick={pickJava}>手动选择</button>
        {#if s.javaPath}<button class="btn sm ghost" onclick={() => save({ javaPath: null })}>改为自动</button>{/if}
      </div>
    </div>
    <div class="row">
      <div class="desc">
        <strong>自动下载 Java</strong>
        <span class="help">找不到合适的 Java 时，从 Mojang 官方下载对应版本。</span>
      </div>
      <Toggle checked={s.autoDownloadJava} onchange={(v) => save({ autoDownloadJava: v })} />
    </div>
    <div class="javas">
      <div class="javas-head">
        <span class="kicker">已检测到的 Java</span>
        <button class="btn ghost sm" onclick={() => scan(true)} disabled={scanning}>
          <span class:spin={scanning}><Icon name="refresh" size={12} /></span> 重新扫描
        </button>
      </div>
      {#if javas === null}
        <p class="muted small">扫描中…</p>
      {:else if javas.length === 0}
        <p class="muted small">没有找到已安装的 Java，启动时会自动下载。</p>
      {:else}
        {#each javas as j}
          <button class="java" class:active={s.javaPath === j.path} onclick={() => save({ javaPath: j.path })}>
            <span class="major">Java {j.major}</span>
            <span class="mono small">{j.version} · {j.arch}</span>
            <span class="mono small path">{j.path}</span>
          </button>
        {/each}
      {/if}
    </div>
  </section>

  <section>
    <h3>内存</h3>
    <div class="row">
      <div class="desc">
        <strong>自动分配</strong>
        <span class="help">根据本机内存和是否安装 Mod 自动决定，推荐开启。</span>
      </div>
      <Toggle checked={s.memoryAuto} onchange={(v) => save({ memoryAuto: v })} />
    </div>
    {#if !s.memoryAuto}
      <div class="row">
        <div class="desc">
          <strong>最大内存</strong>
          <span class="help">本机共 {Math.round(total / 1024)} GB，建议不超过一半。</span>
        </div>
        <div class="ctl range">
          <input
            type="range"
            min="1024"
            max={Math.max(2048, Math.floor(total / 256) * 256)}
            step="256"
            bind:value={s.maxMemoryMb}
            onchange={() => save({ maxMemoryMb: s.maxMemoryMb })}
          />
          <span class="mono val">{(s.maxMemoryMb / 1024).toFixed(2)} GB</span>
        </div>
      </div>
    {/if}
    <div class="row">
      <div class="desc"><strong>最小内存（MB）</strong></div>
      <input class="input num" type="number" min="0" step="256" bind:value={s.minMemoryMb} onchange={() => save({ minMemoryMb: s.minMemoryMb })} />
    </div>
  </section>

  <section>
    <h3>游戏</h3>
    <div class="row">
      <div class="desc">
        <strong>启动后进入服务器</strong>
        <span class="help">1.20 及以上使用 Quick Play，旧版本使用 --server 参数。</span>
      </div>
      <input class="input" bind:value={s.server} onchange={() => save({ server: s.server })} placeholder="mc.example.com:25565" />
    </div>
    <div class="row">
      <div class="desc"><strong>自定义窗口大小</strong></div>
      <div class="ctl">
        <Toggle checked={s.customResolution} onchange={(v) => save({ customResolution: v })} />
        {#if s.customResolution}
          <input class="input num" type="number" bind:value={s.windowWidth} onchange={() => save({ windowWidth: s.windowWidth })} />
          <span class="muted">×</span>
          <input class="input num" type="number" bind:value={s.windowHeight} onchange={() => save({ windowHeight: s.windowHeight })} />
        {/if}
      </div>
    </div>
    <div class="row">
      <div class="desc">
        <strong>默认使用简体中文</strong>
        <span class="help">实例第一次启动时把游戏语言设为简体中文；之后在游戏里改过的语言不会被覆盖。</span>
      </div>
      <Toggle checked={s.defaultChinese} onchange={(v) => save({ defaultChinese: v })} />
    </div>
    <div class="row">
      <div class="desc"><strong>全屏启动</strong></div>
      <Toggle checked={s.fullscreen} onchange={(v) => save({ fullscreen: v })} />
    </div>
    <div class="row">
      <div class="desc"><strong>游戏启动后</strong><span class="help">隐藏时，游戏退出后启动器会重新出现。</span></div>
      <Segmented
        value={s.afterLaunch}
        options={[
          { value: "keep", label: "保持" },
          { value: "hide", label: "隐藏" },
          { value: "close", label: "关闭" },
        ]}
        onchange={(v) => save({ afterLaunch: v })}
      />
    </div>
  </section>

  <section>
    <h3>账号与资源</h3>
    <div class="row col">
      <div class="desc">
        <strong>微软登录 Client ID</strong>
        <span class="help">Azure 应用的「应用程序(客户端) ID」。留空则使用启动器内置的值；如何申请见 README。</span>
      </div>
      <input
        class="input mono wide"
        bind:value={s.msClientId}
        onchange={() => save({ msClientId: s.msClientId.trim() })}
        placeholder="使用内置 Client ID"
      />
    </div>
    <div class="row col">
      <div class="desc">
        <strong>CurseForge API Key</strong>
        <span class="help">留空时通过 MCIM 镜像访问 CurseForge（无需 Key）；填写后可直连官方接口，镜像作为回退。</span>
      </div>
      <input
        class="input mono wide"
        type="password"
        bind:value={s.curseforgeApiKey}
        onchange={() => save({ curseforgeApiKey: s.curseforgeApiKey.trim() })}
        placeholder="使用 MCIM 镜像"
      />
    </div>
  </section>

  <section>
    <button class="fold" onclick={() => (advanced = !advanced)} aria-expanded={advanced}>
      <h3>高级选项</h3>
      <span class="chev" class:open={advanced}><Icon name="chevron" size={14} /></span>
    </button>
    {#if advanced}
      <div class="adv" transition:page={{ y: -6, duration: 180 }}>
        <div class="row col">
          <div class="desc">
            <strong>Java 虚拟机参数</strong>
            <span class="help">追加在启动器默认参数之后。多个参数用空格分隔，含空格的参数用双引号括起来；指定了 GC 参数时会忽略下面的内存回收器设置。</span>
          </div>
          <textarea class="input wide" rows="3" bind:value={s.jvmArgs} onchange={() => save({ jvmArgs: s.jvmArgs })} placeholder="（无）"></textarea>
        </div>
        <div class="row col">
          <div class="desc"><strong>游戏参数</strong><span class="help">追加在游戏启动参数末尾。</span></div>
          <input class="input mono wide" bind:value={s.gameArgs} onchange={() => save({ gameArgs: s.gameArgs })} placeholder="（无）" />
        </div>
        <div class="row col">
          <div class="desc">
            <strong>启动前执行命令</strong>
            <span class="help">
              在游戏目录中执行，可使用占位符 <span class="mono">{"{instance}"}</span> 实例名、<span class="mono">{"{mc}"}</span> 游戏版本、<span class="mono">{"{game_dir}"}</span> 游戏目录、<span class="mono">{"{java}"}</span> Java 路径。
            </span>
          </div>
          <input
            class="input mono wide"
            bind:value={s.preLaunchCommand}
            onchange={() => save({ preLaunchCommand: s.preLaunchCommand })}
            placeholder="（无）"
          />
          <Toggle checked={s.preLaunchWait} onchange={(v) => save({ preLaunchWait: v })} label="等待命令执行完成后再启动游戏（失败时取消启动）" />
        </div>
        <div class="row">
          <div class="desc">
            <strong>内存回收器</strong>
            <span class="help">分代 ZGC 停顿更短，适合大型整合包，需要 Java 21 及以上，不满足时自动使用 G1。</span>
          </div>
          <select class="input" value={s.gc} onchange={(e) => save({ gc: e.currentTarget.value as Settings["gc"] })}>
            <option value="auto">自动（G1）</option>
            <option value="g1">G1</option>
            <option value="zgc">尽量使用分代 ZGC</option>
            <option value="none">不指定（由 Java 决定）</option>
          </select>
        </div>
        <div class="row">
          <div class="desc">
            <strong>使用高性能显卡</strong>
            <span class="help">仅 Windows：在系统「图形设置」中把游戏使用的 Java 设为高性能，双显卡笔记本可以避免用核显运行。</span>
          </div>
          <Toggle checked={s.highPerformanceGpu} onchange={(v) => save({ highPerformanceGpu: v })} />
        </div>
      </div>
    {/if}
  </section>

  <section>
    <h3>外观</h3>
    <div class="row">
      <div class="desc"><strong>主题</strong></div>
      <Segmented
        value={s.theme}
        options={[
          { value: "system", label: "跟随系统" },
          { value: "light", label: "浅色" },
          { value: "dark", label: "深色" },
        ]}
        onchange={(v) => save({ theme: v })}
      />
    </div>
    <div class="row">
      <div class="desc"><strong>主题色</strong></div>
      <div class="ctl swatches">
        {#each accents as c}
          <button
            class="swatch"
            class:active={s.accent.toLowerCase() === c.toLowerCase()}
            style:background={c}
            title={c}
            aria-label={`主题色 ${c}`}
            onclick={() => save({ accent: c })}
          ></button>
        {/each}
        <input type="color" class="picker" value={s.accent} onchange={(e) => save({ accent: e.currentTarget.value })} title="自定义颜色" />
      </div>
    </div>
    <BackgroundPicker />
  </section>

  <section>
    <h3>关于</h3>
    <div class="row">
      <div class="desc">
        <strong>{app.info?.name} {app.info?.version}</strong>
        <span class="help">配置目录：<span class="mono">{app.info?.configDir}</span></span>
      </div>
    </div>
  </section>
</div>

<style>
  .sections {
    display: flex;
    flex-direction: column;
    gap: 26px;
  }
  h3 {
    font-size: 17px;
    padding-bottom: 6px;
    border-bottom: 1px solid var(--rule-strong);
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 24px;
    padding: 12px 0;
    border-bottom: 1px solid var(--rule);
  }
  .row.col {
    flex-direction: column;
    align-items: stretch;
    gap: 8px;
  }
  .desc {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .desc strong {
    font-size: 13px;
  }
  .help {
    font-size: 12px;
    color: var(--muted);
  }
  .path {
    font-size: 12px;
    color: var(--ink-2);
    word-break: break-all;
  }
  .ctl {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: none;
  }
  .range input {
    width: 200px;
    accent-color: var(--accent);
  }
  .val {
    min-width: 70px;
    text-align: right;
    font-size: 12px;
  }
  select.input {
    min-width: 260px;
  }
  .num {
    width: 96px;
  }
  .wide {
    width: 100%;
  }
  .javas {
    padding: 10px 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .javas-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .java {
    display: grid;
    grid-template-columns: 80px 160px minmax(0, 1fr);
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    border: 1px solid var(--rule);
    background: var(--card);
    text-align: left;
    cursor: pointer;
    transition: transform var(--t-fast) var(--ease);
  }
  .java:hover {
    border-color: var(--ink);
  }
  .java:active {
    transform: scale(0.995);
  }
  .java.active {
    border-color: var(--accent);
    box-shadow: inset 4px 0 0 var(--accent);
  }
  .major {
    font-family: var(--serif);
    font-weight: 700;
  }
  .small {
    font-size: 11px;
  }
  .java .path {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--muted);
  }
  .swatches {
    gap: 6px;
  }
  .swatch {
    width: 24px;
    height: 24px;
    border: 1.5px solid var(--ink);
    cursor: pointer;
    transition: transform var(--t-fast) var(--ease);
  }
  .swatch:hover {
    transform: translateY(-2px);
  }
  .swatch.active {
    transform: translateY(-2px);
    box-shadow: 0 4px 0 -1px var(--ink);
  }
  .fold {
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: space-between;
    border: 0;
    border-bottom: 1px solid var(--rule-strong);
    background: transparent;
    padding: 0;
    cursor: pointer;
    text-align: left;
  }
  .fold h3 {
    border: 0;
  }
  .chev {
    transform: rotate(90deg);
    transition: transform var(--t-mid) var(--ease-out);
  }
  .chev.open {
    transform: rotate(-90deg);
  }
  .picker {
    width: 30px;
    height: 26px;
    border: 1.5px solid var(--ink);
    padding: 0;
    background: none;
    cursor: pointer;
  }
</style>
