<script lang="ts">
  // 微软设备码登录：显示验证码，用户在浏览器中登录后自动完成。
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { api, AppError, type MsLoginStart } from "../lib/api";
  import { refreshAccounts, toast } from "../lib/state.svelte";
  import { untrack } from "svelte";
  import Modal from "./Modal.svelte";
  import ProgressBar from "./ProgressBar.svelte";

  let { open = $bindable(false) }: { open: boolean } = $props();

  let start = $state<MsLoginStart | null>(null);
  let phase = $state<"requesting" | "waiting" | "done" | "error">("requesting");
  let error = $state<{ message: string; hint: string } | null>(null);
  let copied = $state(false);
  let remaining = $state(0);
  let timer: ReturnType<typeof setInterval> | undefined;

  $effect(() => {
    if (open) untrack(() => begin());
    return () => stop();
  });

  function stop() {
    if (timer) clearInterval(timer);
    timer = undefined;
    if (start && phase === "waiting") api.msLoginCancel(start.flowId).catch(() => {});
  }

  async function begin() {
    phase = "requesting";
    error = null;
    copied = false;
    start = null;
    try {
      const s = await api.msLoginStart();
      start = s;
      phase = "waiting";
      remaining = s.expiresIn;
      const deadline = Date.now() + s.expiresIn * 1000;
      timer = setInterval(() => (remaining = Math.max(0, Math.round((deadline - Date.now()) / 1000))), 1000);
      const account = await api.msLoginFinish(s.flowId);
      phase = "done";
      if (timer) clearInterval(timer);
      await refreshAccounts();
      toast("ok", `已登录正版账号 ${account.name}`);
      open = false;
    } catch (e) {
      if (!open) return;
      if (timer) clearInterval(timer);
      const err = e instanceof AppError ? e : new AppError({ message: String(e), hint: "" });
      if (err.message === "操作已取消") return;
      phase = "error";
      error = { message: err.message, hint: err.hint };
    }
  }

  async function copyAndOpen() {
    if (!start) return;
    try {
      await navigator.clipboard.writeText(start.userCode);
      copied = true;
    } catch {
      copied = false;
    }
    await openUrl(start.verificationUri).catch(() => {});
  }

  const mmss = $derived(`${Math.floor(remaining / 60)}:${String(remaining % 60).padStart(2, "0")}`);
</script>

<Modal bind:open title="微软正版登录" kicker="MICROSOFT" width={500}>
  {#if phase === "requesting"}
    <p class="muted">正在向微软申请验证码…</p>
    <div class="gap"><ProgressBar indeterminate size="sm" /></div>
  {:else if phase === "waiting" && start}
    <ol class="steps">
      <li>点击下方按钮，验证码会自动复制，并在浏览器中打开 <span class="mono">{start.verificationUri}</span></li>
      <li>粘贴验证码，登录购买了 Minecraft 的微软账号，并允许访问</li>
      <li>回到启动器，登录会自动完成</li>
    </ol>
    <div class="code mono" aria-label="验证码">
      {#each start.userCode.split("") as ch, i}
        <span style:--i={i}>{ch}</span>
      {/each}
    </div>
    <div class="row">
      <button class="btn primary" onclick={copyAndOpen}>{copied ? "已复制，再次打开网页" : "复制验证码并打开网页"}</button>
      <span class="muted mono">剩余 {mmss}</span>
    </div>
    <div class="gap"><ProgressBar indeterminate size="sm" /></div>
    <p class="muted small">等待你在浏览器中完成登录…</p>
  {:else if phase === "error" && error}
    <div class="err">
      <strong>{error.message}</strong>
      {#if error.hint}<p>{error.hint}</p>{/if}
    </div>
  {/if}
  {#snippet actions()}
    {#if phase === "error"}
      <button class="btn" onclick={() => (open = false)}>关闭</button>
      <button class="btn primary" onclick={begin}>重试</button>
    {:else}
      <button class="btn" onclick={() => (open = false)}>取消</button>
    {/if}
  {/snippet}
</Modal>

<style>
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
