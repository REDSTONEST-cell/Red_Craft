<script lang="ts">
  // 微软登录：默认在浏览器中确认后自动完成；也可以改用验证码登录。
  import { untrack } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { api, AppError, type MsLoginStart } from "../lib/api";
  import { refreshAccounts, toast } from "../lib/state.svelte";
  import Modal from "./Modal.svelte";
  import ProgressBar from "./ProgressBar.svelte";

  let { open = $bindable(false) }: { open: boolean } = $props();

  type Mode = "browser" | "code";
  let mode = $state<Mode>("browser");
  let phase = $state<"starting" | "waiting" | "error">("starting");
  let error = $state<{ message: string; hint: string } | null>(null);
  let flowId: number | null = null;
  let browserUrl = $state("");
  let code = $state<MsLoginStart | null>(null);
  let copied = $state(false);
  let remaining = $state(0);
  let timer: ReturnType<typeof setInterval> | undefined;
  let run = 0;

  $effect(() => {
    if (open) untrack(() => begin("browser"));
    return () => stop();
  });

  function stop() {
    run++;
    if (timer) clearInterval(timer);
    timer = undefined;
    if (flowId !== null) api.msLoginCancel(flowId).catch(() => {});
    flowId = null;
  }

  async function finished(name: string) {
    await refreshAccounts();
    toast("ok", `已登录正版账号 ${name}`);
    open = false;
  }

  function failed(e: unknown, current: number) {
    if (current !== run || !open) return;
    const err = e instanceof AppError ? e : new AppError({ message: String(e), hint: "" });
    if (err.message === "操作已取消") return;
    if (timer) clearInterval(timer);
    phase = "error";
    error = { message: err.message, hint: err.hint };
  }

  async function begin(m: Mode) {
    stop();
    const current = run;
    mode = m;
    phase = "starting";
    error = null;
    code = null;
    copied = false;
    try {
      if (m === "browser") {
        const s = await api.msBrowserStart();
        if (current !== run) return;
        flowId = s.flowId;
        browserUrl = s.url;
        phase = "waiting";
        const account = await api.msBrowserFinish(s.flowId);
        flowId = null;
        await finished(account.name);
      } else {
        const s = await api.msLoginStart();
        if (current !== run) return;
        flowId = s.flowId;
        code = s;
        phase = "waiting";
        remaining = s.expiresIn;
        const deadline = Date.now() + s.expiresIn * 1000;
        timer = setInterval(() => (remaining = Math.max(0, Math.round((deadline - Date.now()) / 1000))), 1000);
        const account = await api.msLoginFinish(s.flowId);
        flowId = null;
        if (timer) clearInterval(timer);
        await finished(account.name);
      }
    } catch (e) {
      flowId = null;
      failed(e, current);
    }
  }

  async function copyAndOpen() {
    if (!code) return;
    try {
      await navigator.clipboard.writeText(code.userCode);
      copied = true;
    } catch {
      copied = false;
    }
    await openUrl(code.verificationUri).catch(() => {});
  }

  const mmss = $derived(`${Math.floor(remaining / 60)}:${String(remaining % 60).padStart(2, "0")}`);
</script>

<Modal bind:open title="微软正版登录" kicker="MICROSOFT" width={500}>
  {#if phase === "starting"}
    <p class="muted">{mode === "browser" ? "正在打开浏览器…" : "正在向微软申请验证码…"}</p>
    <div class="gap"><ProgressBar indeterminate size="sm" /></div>
  {:else if phase === "waiting" && mode === "browser"}
    <div class="lead">
      <span class="pixel"></span>
      <div>
        <strong>请在浏览器中完成登录</strong>
        <p class="muted">选择购买了 Minecraft 的微软账号并点击「接受」，登录会自动完成，然后可以关闭浏览器页面。</p>
      </div>
    </div>
    <div class="gap"><ProgressBar indeterminate size="sm" /></div>
    <p class="muted small">
      浏览器没有打开？<button class="link" onclick={() => openUrl(browserUrl)}>点这里重新打开</button>
      · 浏览器提示错误？<button class="link" onclick={() => begin("code")}>改用验证码登录</button>
    </p>
  {:else if phase === "waiting" && code}
    <ol class="steps">
      <li>点击下方按钮，验证码会自动复制，并在浏览器中打开 <span class="mono">{code.verificationUri}</span></li>
      <li>粘贴验证码，登录购买了 Minecraft 的微软账号，并允许访问</li>
      <li>回到启动器，登录会自动完成</li>
    </ol>
    <div class="code mono" aria-label="验证码">
      {#each code.userCode.split("") as ch, i}
        <span style:--i={i}>{ch}</span>
      {/each}
    </div>
    <div class="row">
      <button class="btn primary" onclick={copyAndOpen}>{copied ? "已复制，再次打开网页" : "复制验证码并打开网页"}</button>
      <span class="muted mono">剩余 {mmss}</span>
    </div>
    <div class="gap"><ProgressBar indeterminate size="sm" /></div>
    <p class="muted small"><button class="link" onclick={() => begin("browser")}>改用浏览器直接登录</button></p>
  {:else if phase === "error" && error}
    <div class="err">
      <strong>{error.message}</strong>
      {#if error.hint}<p>{error.hint}</p>{/if}
    </div>
  {/if}
  {#snippet actions()}
    {#if phase === "error"}
      <button class="btn" onclick={() => begin(mode === "browser" ? "code" : "browser")}>
        {mode === "browser" ? "改用验证码登录" : "改用浏览器登录"}
      </button>
      <button class="btn primary" onclick={() => begin(mode)}>重试</button>
    {:else}
      <button class="btn" onclick={() => (open = false)}>取消</button>
    {/if}
  {/snippet}
</Modal>

<style>
  .lead {
    display: flex;
    gap: 12px;
    align-items: flex-start;
  }
  .lead .pixel {
    margin-top: 6px;
  }
  .lead p {
    font-size: 13px;
    margin-top: 4px;
  }
  .link {
    border: 0;
    background: none;
    padding: 0;
    color: var(--accent);
    cursor: pointer;
    font: inherit;
  }
  .link:hover {
    text-decoration: underline;
  }
  .steps {
    margin: 0 0 16px;
    padding-left: 20px;
    font-size: 13px;
    color: var(--ink-2);
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .code {
    display: flex;
    justify-content: center;
    gap: 6px;
    margin: 6px 0 16px;
    user-select: text;
  }
  .code span {
    width: 40px;
    height: 50px;
    display: grid;
    place-items: center;
    font-size: 26px;
    font-weight: 700;
    border: 1.5px solid var(--ink);
    background: var(--paper);
    box-shadow: var(--shadow-sm);
    animation: rise var(--t-slow) var(--ease-out) both;
    animation-delay: calc(var(--i) * 40ms);
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }
  .gap {
    margin: 14px 0 6px;
  }
  .small {
    font-size: 12px;
  }
  .err {
    border-left: 4px solid var(--danger);
    padding: 10px 12px;
    background: color-mix(in srgb, var(--danger) 8%, transparent);
  }
  .err p {
    margin-top: 4px;
    font-size: 13px;
    color: var(--ink-2);
    user-select: text;
  }
</style>
