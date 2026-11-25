<script lang="ts">
  // 联机：房主创建房间并把房间号发给朋友，朋友输入房间号加入。由陶瓦联机（Terracotta）提供，与 HMCL、PCL CE 互通。
  import { onMount } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { api } from "../lib/api";
  import { app, selectedAccount, selectedInstance, showError, toast, waitTask } from "../lib/state.svelte";
  import { multiplayerApi, normalizeRoom, exceptionTips, describeLink, DIFFICULTY, EXCEPTION, type Diagnosis, type Link, type MultiplayerInfo, type MultiplayerState, type RoomMember } from "../lib/multiplayer";
  import Icon from "../components/Icon.svelte";

  let info = $state<MultiplayerInfo | null>(null);
  let current = $state<MultiplayerState>({ state: "stopped" });
  let busy = $state(false);
  let roomInput = $state("");
  const room = $derived(normalizeRoom(roomInput));
  const account = $derived(selectedAccount());
  const inst = $derived(selectedInstance());
  const player = $derived(account?.name ?? null);
  // 陶瓦每次状态变化 index 递增；较旧的响应直接忽略
  let lastIndex = -1;

  async function poll() {
    try {
      const next = await multiplayerApi.state();
      const index = "index" in next && typeof next.index === "number" ? next.index : -1;
      if (next.state === "stopped" || index >= lastIndex || index === 0) {
        lastIndex = index;
        current = next;
      }
    } catch {
      /* 轮询失败时保持上一次的状态 */
    }
  }

  onMount(() => {
    multiplayerApi.info().then((v) => (info = v)).catch((e) => showError(e));
    void poll();
    const timer = setInterval(() => {
      if (!document.hidden) void poll();
    }, 700);
    return () => clearInterval(timer);
  });

  async function run(action: () => Promise<unknown>, failure: string) {
    busy = true;
    try {
      await action();
      lastIndex = -1;
      await poll();
    } catch (e) {
      showError(e, failure);
    } finally {
      busy = false;
    }
  }

  const host = () => run(() => multiplayerApi.host(player), "创建房间失败");
  const join = () => room && run(() => multiplayerApi.join(room, player), "加入房间失败");
  const leave = () => run(() => multiplayerApi.leave(), "操作失败");

  // 出错时读取陶瓦日志，找出具体原因（例如端口被系统保留）
  let diagnosis = $state<Diagnosis | null>(null);
  let diagnosedIndex = -1;
  $effect(() => {
    const now = current;
    if (now.state !== "exception") {
      diagnosis = null;
      diagnosedIndex = -1;
      return;
    }
    if (now.index === diagnosedIndex) return;
    diagnosedIndex = now.index;
    multiplayerApi.diagnose().then((d) => (diagnosis = d)).catch(() => (diagnosis = null));
  });

  // 加入成功后显示与房主之间走的是 IPv4 还是 IPv6（连接可能随打洞结果变化，定时刷新）
  let link = $state<Link | null>(null);
  const joined = $derived(current.state === "guest-ok");
  $effect(() => {
    if (!joined) {
      link = null;
      return;
    }
    let alive = true;
    const refresh = () => {
      if (document.hidden) return;
      multiplayerApi.link().then((l) => alive && (link = l)).catch(() => {});
    };
    refresh();
    const timer = setInterval(refresh, 5000);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  });

  let exporting = $state(false);
  async function exportLog() {
    exporting = true;
    try {
      const file = await multiplayerApi.exportLog();
      toast("ok", "已导出联机日志", file);
    } catch (e) {
      showError(e, "导出联机日志失败");
    } finally {
      exporting = false;
    }
  }

  async function copy(text: string, what: string) {
    try {
      await navigator.clipboard.writeText(text);
      toast("ok", `已复制${what}`, text);
    } catch (e) {
      showError(e, "复制失败");
    }
  }

  async function paste() {
    try {
      roomInput = (await navigator.clipboard.readText()).trim();
    } catch {
      /* 没有剪贴板权限时手动输入 */
    }
  }

  // 房客：用当前版本启动游戏并直接进入房间
  let launching = $state(false);
  async function launchAndJoin(url: string) {
    if (!inst) {
      app.page = "instances";
      return;
    }
    if (!account) {
      app.page = "accounts";
      return;
    }
    launching = true;
    try {
      const task = await api.launch(inst.id, url);
      toast("info", `正在启动「${inst.id}」`, "进入游戏后会自动连接到房间");
      await waitTask(task);
    } catch (e) {
      showError(e, "启动失败");
    } finally {
      launching = false;
    }
  }

  const KIND_LABEL: Record<RoomMember["kind"], string> = { HOST: "房主", LOCAL: "我", GUEST: "成员" };
  const vendorOf = (m: RoomMember) => m.vendor.split(",")[0];
</script>

<div class="page-head">
  <div>
    <div class="kicker">MULTIPLAYER</div>
    <h1>联机</h1>
    <div class="sub">房主创建房间并把房间号发给朋友，朋友输入房间号就能加入</div>
  </div>
</div>

{#if info && !info.available}
  <div class="empty"><h3>无法使用联机</h3><p>{info.reason}</p></div>
{:else if info?.needsInstall}
  <div class="empty">
    <h3>需要安装联机组件</h3>
    <p>在 macOS 上，陶瓦联机需要通过官方安装包安装（会请求管理员授权与本地网络权限）。安装完成后回到这里即可使用。</p>
    <button class="btn primary" style="margin-top:14px" onclick={() => run(() => multiplayerApi.install(), "无法打开安装包")}>安装联机组件</button>
  </div>
{:else if current.state === "stopped" || current.state === "waiting"}
  <div class="choices">
    <section class="card">
      <div class="kicker">我当房主</div>
      <h2>创建房间</h2>
      <ol>
        <li>启动游戏，进入要一起玩的存档</li>
        <li>按 Esc，选择「对局域网开放」，建议把「端口号」设为 25565，再点「创建局域网世界」</li>
        <li>回到这里点「创建房间」，把生成的房间号发给朋友</li>
      </ol>
      <button class="btn primary" disabled={busy} onclick={host}><Icon name="plus" size={14} /> 创建房间</button>
    </section>
    <section class="card">
      <div class="kicker">我去朋友家</div>
      <h2>加入房间</h2>
      <p class="help">输入房主发来的房间号，形如 U/XXXX-XXXX-XXXX-XXXX。</p>
      <div class="join">
        <input class="input mono" placeholder="U/XXXX-XXXX-XXXX-XXXX" bind:value={roomInput} maxlength="40"
          onkeydown={(e) => e.key === "Enter" && join()} />
        <button class="btn" title="从剪贴板粘贴" onclick={paste}>粘贴</button>
      </div>
      {#if roomInput.trim() && !room}<p class="warn">房间号格式不正确，请检查是否输错。</p>{/if}
      <button class="btn primary" disabled={busy || !room} onclick={join}><Icon name="link" size={14} /> 加入房间</button>
    </section>
  </div>
{:else}
  <section class="card status">
    {#if current.state === "host-scanning"}
      <div class="kicker">创建房间</div>
      <h2>正在寻找对局域网开放的游戏…</h2>
      <p class="help">请在游戏中按 Esc →「对局域网开放」→「创建局域网世界」。找到后会自动生成房间号。</p>
      <div class="spinner" aria-hidden="true"></div>
      <div class="actions"><button class="btn" disabled={busy} onclick={leave}>取消</button></div>
    {:else if current.state === "host-starting" || current.state === "host-ok"}
      {@const code = current.room}
      <div class="kicker">{current.state === "host-ok" ? "房间已开放" : "正在创建房间"}</div>
      <div class="code">
        <span class="mono">{current.room}</span>
        <button class="btn" onclick={() => copy(code, "房间号")}><Icon name="check" size={14} /> 复制房间号</button>
      </div>
      <p class="help">把房间号发给朋友，让他们在 RCL、HMCL 或 PCL CE 的联机中输入。关闭游戏或退出启动器后房间会关闭。</p>
      {#if current.state === "host-ok"}
        <h3>房间成员（{current.profiles.length}）</h3>
        <ul class="members">
          {#each current.profiles as m (m.machine_id)}
            <li><strong>{m.name}</strong><span class="tag">{KIND_LABEL[m.kind]}</span><span class="muted">{vendorOf(m)}</span></li>
          {/each}
        </ul>
      {:else}
        <div class="spinner" aria-hidden="true"></div>
      {/if}
      <div class="actions"><button class="btn danger" disabled={busy} onclick={leave}>关闭房间</button></div>
    {:else if current.state === "guest-connecting" || current.state === "guest-starting"}
      <div class="kicker">加入房间</div>
      <h2>正在连接房间…</h2>
      <p class="help">{current.state === "guest-starting" ? DIFFICULTY[current.difficulty] : "正在寻找房主"}{current.room ? `（${current.room}）` : ""}</p>
      <div class="spinner" aria-hidden="true"></div>
      <div class="actions"><button class="btn" disabled={busy} onclick={leave}>取消</button></div>
    {:else if current.state === "guest-ok"}
      {@const url = current.url}
      <div class="kicker">已加入房间</div>
      <h2>连接成功</h2>
      <p class="route" class:direct={link && !link.relayed}>
        <span class="dot" aria-hidden="true"></span>连接方式：{link ? describeLink(link) : "检测中…"}
      </p>
      <p class="help">游戏「多人游戏」页面的局域网列表里会出现这个房间；也可以用「直接连接」输入下面的地址。</p>
      <div class="code">
        <span class="mono">{current.url}</span>
        <button class="btn" onclick={() => copy(url, "地址")}><Icon name="check" size={14} /> 复制地址</button>
      </div>
      <div class="actions">
        <button class="btn primary" disabled={launching || !inst} onclick={() => launchAndJoin(url)}>
          <Icon name="play" size={14} /> {launching ? "正在启动…" : inst ? `用「${inst.id}」启动并进入` : "请先选择版本"}
        </button>
        <button class="btn" disabled={busy} onclick={leave}>离开房间</button>
      </div>
      <h3>房间成员（{current.profiles.length}）</h3>
      <ul class="members">
        {#each current.profiles as m (m.machine_id)}
          <li><strong>{m.name}</strong><span class="tag">{KIND_LABEL[m.kind]}</span><span class="muted">{vendorOf(m)}</span></li>
        {/each}
      </ul>
    {:else if current.state === "exception"}
      <div class="kicker">联机中断</div>
      <h2>{current.type === 2 && diagnosis?.portBlocked ? "无法在本机建立游戏端口的转发" : EXCEPTION[current.type] ?? "联机出现了问题"}</h2>
      <ul class="tips">
        {#each exceptionTips(current.type, diagnosis) as tip}<li>{tip}</li>{/each}
      </ul>
      <div class="actions">
        <button class="btn primary" disabled={busy} onclick={leave}>返回</button>
        <button class="btn" disabled={exporting} onclick={exportLog}>{exporting ? "正在导出…" : "导出联机日志"}</button>
      </div>
    {/if}
  </section>
{/if}

<!-- 陶瓦联机许可证要求：在界面显著位置标明其版权信息 -->
<footer class="credit">
  联机功能由 <button type="button" class="link" onclick={() => openUrl("https://github.com/burningtnt/Terracotta")}>Terracotta | 陶瓦联机</button>
  {info?.version ? `v${info.version}` : ""} 提供 · Copyright © burningtnt · AGPL-3.0-or-later · 基于
  <button type="button" class="link" onclick={() => openUrl("https://github.com/EasyTier/EasyTier")}>EasyTier</button>
  · 公共中继节点由 zkitefly 提供 · 可与 HMCL、PCL CE 的联机互通
  · <button type="button" class="link" disabled={exporting} onclick={exportLog}>导出联机日志</button>
</footer>

<style>
  .choices {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
    gap: 16px;
  }
  .card {
    padding: 18px 20px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    align-items: flex-start;
  }
  .card h2 {
    font-size: 22px;
  }
  .card h3 {
    font-size: 14px;
    margin-top: 8px;
  }
  ol {
    margin: 0;
    padding-left: 20px;
    font-size: 13px;
    line-height: 1.8;
  }
  .help {
    font-size: 13px;
    color: var(--muted);
  }
  .warn {
    font-size: 12px;
    color: var(--danger);
  }
  .join {
    display: flex;
    gap: 8px;
    width: 100%;
  }
  .join .input {
    flex: 1;
    letter-spacing: 0.04em;
  }
  .status {
    max-width: 720px;
  }
  .route {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 0 8px;
    font-size: 13px;
    color: var(--muted);
  }
  .route .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: #d99a2b;
  }
  .route.direct .dot {
    background: var(--ok);
  }
  .code {
    display: flex;
    align-items: center;
    gap: 14px;
    flex-wrap: wrap;
  }
  .code .mono {
    font-size: 26px;
    font-weight: 700;
    letter-spacing: 0.06em;
    user-select: text;
  }
  .tips {
    margin: 0;
    padding-left: 20px;
    font-size: 13px;
    line-height: 1.8;
    user-select: text;
  }
  .members {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
    width: 100%;
  }
  .members li {
    display: flex;
    align-items: baseline;
    gap: 10px;
    font-size: 13px;
  }
  .members .muted {
    font-size: 12px;
  }
  .actions {
    display: flex;
    gap: 10px;
    margin-top: 6px;
  }
  .spinner {
    width: 22px;
    height: 22px;
    border-radius: 50%;
    border: 2.5px solid color-mix(in srgb, var(--accent) 30%, transparent);
    border-top-color: var(--accent);
    animation: spin 0.9s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .credit {
    margin-top: 22px;
    font-size: 12px;
    color: var(--muted);
    line-height: 1.7;
  }
  .link {
    border: 0;
    background: none;
    padding: 0;
    color: var(--accent);
    cursor: pointer;
    font: inherit;
  }
</style>
