<script lang="ts">
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { api, type DownloadSource, type JavaInstall, type MemoryStatus, type Settings } from "../lib/api";
  import { app, memoryToast, refreshInstances, saveSettings, showError, toast } from "../lib/state.svelte";
  import Segmented from "../components/Segmented.svelte";
  import Toggle from "../components/Toggle.svelte";
  import Icon from "../components/Icon.svelte";
  import BackgroundPicker from "../components/BackgroundPicker.svelte";
  import { page } from "../lib/motion";
  import { updater, checkUpdate } from "../lib/update.svelte";

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

  // 与 PCL 相同的三种下载源策略
  const sources: { value: DownloadSource; label: string }[] = [
    { value: "bmclapi", label: "尽量使用镜像源" },
    { value: "auto", label: "优先使用官方源，在加载缓慢时换用镜像源" },
    { value: "official", label: "尽量使用官方源" },
  ];

  // 速度限制档位（KB/s），0 为不限速
  const SPEED_STEPS = [256, 512, 1024, 2048, 3072, 5120, 8192, 12288, 20480, 0];
  let speedStep = $state(nearestStep(s.speedLimitKb));
  function nearestStep(kb: number) {
    if (!kb) return SPEED_STEPS.length - 1;
    let best = 0;
    SPEED_STEPS.forEach((v, i) => {
      if (v && Math.abs(v - kb) < Math.abs(SPEED_STEPS[best] - kb)) best = i;
    });
    return best;
  }
  function speedLabel(kb: number) {
    if (!kb) return "不限速";
    return kb >= 1024 ? `${+(kb / 1024).toFixed(1)} MB/s` : `${kb} KB/s`;
  }

  const total = $derived(app.info?.memory.totalMb ?? 8192);

  // 内存使用情况：设置页打开时每 2 秒刷新一次
  let mem = $state<MemoryStatus | null>(null);
  let optimizing = $state(false);
  const gb = (mb: number) => `${(mb / 1024).toFixed(1)} GB`;
  async function refreshMemory() {
    try {
      mem = await api.memoryStatus();
    } catch {
      /* 忽略 */
    }
  }
  $effect(() => {
    void [s.memoryAuto, s.maxMemoryMb, app.settings?.selectedInstance];
    refreshMemory();
    const timer = setInterval(refreshMemory, 2000);
    return () => clearInterval(timer);
  });
  async function optimizeNow() {
    optimizing = true;
    try {
      memoryToast(await api.optimizeMemory(true));
      await refreshMemory();
    } catch (e) {
      showError(e, "内存优化失败");
    } finally {
      optimizing = false;
    }
  }
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
        <span class="help">开启隔离的版本使用 versions/版本名称 作为游戏目录，存档和 Mod 互不干扰。每个版本也可单独设置。</span>
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
        <strong>文件下载源</strong>
        <span class="help">游戏本体、依赖库、资源文件与加载器的下载。镜像源为 BMCLAPI。</span>
      </div>
      <select class="input" value={s.downloadSource} onchange={(e) => save({ downloadSource: e.currentTarget.value as DownloadSource })}>
        {#each sources as o}<option value={o.value}>{o.label}</option>{/each}
      </select>
    </div>
    <div class="row">
      <div class="desc">
        <strong>版本列表源</strong>
        <span class="help">Minecraft 版本、Forge / Fabric 等加载器与 OptiFine 的版本列表。</span>
      </div>
      <select class="input" value={s.versionSource} onchange={(e) => save({ versionSource: e.currentTarget.value as DownloadSource })}>
        {#each sources as o}<option value={o.value}>{o.label}</option>{/each}
      </select>
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
    <div class="row">
      <div class="desc">
        <strong>速度限制</strong>
        <span class="help">限制所有下载任务的总速度，修改后立即生效。</span>
      </div>
      <div class="ctl range">
        <input type="range" min="0" max={SPEED_STEPS.length - 1} step="1" bind:value={speedStep} onchange={() => save({ speedLimitKb: SPEED_STEPS[speedStep] })} />
        <span class="mono val wide-val">{speedLabel(SPEED_STEPS[speedStep])}</span>
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
    {#if mem}
      {@const used = mem.totalMb - mem.availableMb}
      {@const game = Math.min(mem.gameMb, Math.max(0, mem.totalMb - used))}
      <div class="mem">
        <div class="mem-bar" role="img" aria-label="内存使用情况">
          <span class="seg used" style:width="{(used / mem.totalMb) * 100}%"></span>
          <span class="seg game" style:width="{(game / mem.totalMb) * 100}%"></span>
        </div>
        <div class="mem-legend">
          <div style:width="{Math.max(30, (used / mem.totalMb) * 100)}%">
            <span class="k">已使用内存</span>
            <span class="v mono">{gb(used)} / {gb(mem.totalMb)}</span>
          </div>
          <div>
            <span class="k">游戏分配</span>
            <span class="v mono">{gb(mem.gameMb)}</span>
          </div>
        </div>
      </div>
    {/if}
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
      <div class="desc">
        <strong>启动游戏前进行内存优化</strong>
        <span class="help">
          {#if mem && !mem.supported}仅支持 Windows。{:else}启动前整理系统内存，让游戏拿到更多可用内存；以管理员身份运行启动器时效果更好。{/if}
        </span>
      </div>
      <div class="ctl">
        {#if mem?.supported}
          <button class="btn sm" disabled={optimizing} onclick={optimizeNow}>{optimizing ? "优化中…" : "立即优化"}</button>
        {/if}
        <Toggle checked={s.optimizeMemory} disabled={mem ? !mem.supported : false} onchange={(v) => save({ optimizeMemory: v })} />
      </div>
    </div>
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
        <span class="help">每个版本第一次启动时把游戏语言设为简体中文；之后在游戏里改过的语言不会被覆盖。</span>
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
    <div class="row">
      <div class="desc">
        <strong>社区资源来源</strong>
        <span class="help">Modrinth / CurseForge 的搜索、图标与文件下载。镜像源为 MCIM。</span>
      </div>
      <select class="input" value={s.communitySource} onchange={(e) => save({ communitySource: e.currentTarget.value as DownloadSource })}>
        {#each sources as o}<option value={o.value}>{o.label}</option>{/each}
      </select>
    </div>
    <div class="row">
      <div class="desc">
        <strong>文件名格式</strong>
        <span class="help">下载 Mod 时在文件名中加入中文名，方便在文件夹中辨认（仅限收录了译名的 Mod）。</span>
      </div>
      <select class="input" value={s.modFilename} onchange={(e) => save({ modFilename: e.currentTarget.value as Settings["modFilename"] })}>
        <option value="original">create-1.21.1-6.0.4</option>
        <option value="zhPrefix">[机械动力] create-1.21.1-6.0.4</option>
        <option value="zhSuffix">create-1.21.1-6.0.4-机械动力</option>
      </select>
    </div>
    <div class="row">
      <div class="desc">
        <strong>Mod 管理样式</strong>
        <span class="help">已安装 Mod 列表中标题与详情的显示内容。</span>
      </div>
      <select class="input" value={s.modTitle} onchange={(e) => save({ modTitle: e.currentTarget.value as Settings["modTitle"] })}>
        <option value="translated">标题显示译名，详情显示文件名</option>
        <option value="filename">标题显示文件名，详情显示译名</option>
      </select>
    </div>
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
              在游戏目录中执行，可使用占位符 <span class="mono">{"{instance}"}</span> 版本名称、<span class="mono">{"{mc}"}</span> Minecraft 版本号、<span class="mono">{"{game_dir}"}</span> 游戏目录、<span class="mono">{"{java}"}</span> Java 路径。
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
    <div class="row">
      <div class="desc"><strong>形状</strong><span class="help">利落：直角与硬朗线条的报刊风；圆润：圆角与柔和阴影，类似 PCL2；毛玻璃：半透明虚化面板，Apple 风格</span></div>
      <Segmented
        value={s.corner}
        options={[
          { value: "sharp", label: "利落" },
          { value: "rounded", label: "圆润" },
          { value: "glass", label: "毛玻璃" },
        ]}
        onchange={(v) => save({ corner: v })}
      />
    </div>
    {#if s.corner === "glass"}
      <div class="row" transition:page={{ y: 6 }}>
        <div class="desc"><strong>玻璃不透明度</strong><span class="help">数值越低越通透；弹出的窗口始终保持较高不透明度以保证可读性</span></div>
        <div class="ctl range">
          <input
            type="range"
            min="20"
            max="95"
            step="1"
            value={Math.round(s.glassOpacity * 100)}
            oninput={(e) => (app.settings!.glassOpacity = s.glassOpacity = +e.currentTarget.value / 100)}
            onchange={() => save({ glassOpacity: s.glassOpacity })}
          />
          <span class="mono val">{Math.round(s.glassOpacity * 100)}%</span>
        </div>
      </div>
      <div class="row" transition:page={{ y: 6 }}>
        <div class="desc"><strong>玻璃模糊度</strong><span class="help">毛玻璃面板对背后画面的模糊程度；设为 0 时背景图片保持清晰</span></div>
        <div class="ctl range">
          <input
            type="range"
            min="0"
            max="40"
            step="1"
            value={s.glassBlur}
            oninput={(e) => (app.settings!.glassBlur = s.glassBlur = +e.currentTarget.value)}
            onchange={() => save({ glassBlur: s.glassBlur })}
          />
          <span class="mono val">{s.glassBlur}px</span>
        </div>
      </div>
    {/if}
    <div class="row">
      <div class="desc"><strong>布局</strong><span class="help">极简：去掉报头与装饰，只突出主要内容，字号更大</span></div>
      <Segmented
        value={s.layout}
        options={[
          { value: "standard", label: "标准" },
          { value: "minimal", label: "极简" },
        ]}
        onchange={(v) => save({ layout: v })}
      />
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
      <button class="btn sm" disabled={updater.checking} onclick={() => checkUpdate(true)}>
        <span class:spin={updater.checking}><Icon name="refresh" size={12} /></span>
        {updater.checking ? "检查中…" : "检查更新"}
      </button>
    </div>
    <div class="row">
      <div class="desc">
        <strong>自动检查更新</strong>
        <span class="help">启动时检查是否有新的正式版，发现新版本时先弹窗确认，确认后才下载并安装。</span>
      </div>
      <Toggle checked={s.autoCheckUpdate} onchange={(v) => save({ autoCheckUpdate: v, skippedUpdate: "" })} />
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
  .mem {
    padding: 12px 0 14px;
    border-bottom: 1px solid var(--rule);
  }
  .mem-bar {
    display: flex;
    height: 6px;
    border-radius: 3px;
    overflow: hidden;
    background: color-mix(in srgb, var(--accent) 14%, var(--rule));
  }
  .mem-bar .seg {
    height: 100%;
    transition: width var(--t-mid) var(--ease-out);
  }
  .mem-bar .used {
    background: var(--accent);
  }
  .mem-bar .game {
    background: color-mix(in srgb, var(--accent) 45%, transparent);
  }
  .mem-legend {
    display: flex;
    margin-top: 8px;
  }
  .mem-legend > div {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: none;
  }
  .mem-legend .k {
    font-size: 12px;
    color: var(--muted);
  }
  .mem-legend .v {
    font-size: 15px;
  }
  .wide-val {
    min-width: 84px;
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
