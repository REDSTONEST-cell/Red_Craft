<script lang="ts">
  // 外置登录：认证服务器地址 + 账号密码，多角色时选择角色。
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { api, type YggProfile, type YggServerInfo } from "../lib/api";
  import { refreshAccounts, toast } from "../lib/state.svelte";
  import Modal from "./Modal.svelte";
  import Avatar from "./Avatar.svelte";
  import ProgressBar from "./ProgressBar.svelte";

  let { open = $bindable(false) }: { open: boolean } = $props();

  const PRESETS = [
    { label: "LittleSkin", url: "littleskin.cn" },
    { label: "Blessing Skin", url: "skin.prinzeugen.net" },
  ];

  function lastServer(): string {
    try {
      return localStorage.getItem("rc.yggServer") || PRESETS[0].url;
    } catch {
      return PRESETS[0].url;
    }
  }

  let server = $state(lastServer());
  let username = $state("");
  let password = $state("");
  let phase = $state<"form" | "busy" | "choose">("form");
  let error = $state<{ message: string; hint: string } | null>(null);
  let info = $state<YggServerInfo | null>(null);
  let checking = $state(false);
  let choice = $state<{ flowId: number; serverName: string; profiles: YggProfile[] } | null>(null);

  // 打开时重置
  $effect(() => {
    if (open) {
      phase = "form";
      error = null;
      password = "";
      choice = null;
    }
  });

  // 输入地址后检查服务器，显示服务器名称
  let gen = 0;
  $effect(() => {
    const s = server.trim();
    if (!open || !s) {
      info = null;
      return;
    }
    const g = ++gen;
    checking = true;
    const t = setTimeout(() => {
      api
        .yggResolve(s)
        .then((r) => g === gen && (info = r))
        .catch(() => g === gen && (info = null))
        .finally(() => g === gen && (checking = false));
    }, 500);
    return () => clearTimeout(t);
  });

  async function login() {
    phase = "busy";
    error = null;
    try {
      const r = await api.yggLogin(server.trim(), username.trim(), password);
      password = "";
      try {
        localStorage.setItem("rc.yggServer", server.trim());
      } catch {
        /* 忽略 */
      }
      if (r.type === "done") {
        await done(r.account.name);
      } else {
        choice = r;
        phase = "choose";
      }
    } catch (e) {
      error = e as { message: string; hint: string };
      phase = "form";
    }
  }

  async function pick(p: YggProfile) {
    if (!choice) return;
    phase = "busy";
    try {
      const a = await api.yggSelect(choice.flowId, p.id);
      await done(a.name);
    } catch (e) {
      error = e as { message: string; hint: string };
      phase = "form";
    }
  }

  async function done(name: string) {
    await refreshAccounts();
    toast("ok", `已登录外置账号 ${name}`);
    open = false;
  }

  function cancel() {
    if (choice) api.yggCancel(choice.flowId).catch(() => {});
    open = false;
  }
</script>

<Modal bind:open title="外置登录" kicker="AUTHLIB-INJECTOR" width={480} onclose={cancel}>
  {#if phase === "choose" && choice}
    <p class="muted lead">这个账号在「{choice.serverName}」上有多个角色，请选择要使用的角色：</p>
    <div class="profiles stagger">
      {#each choice.profiles as p, i (p.id)}
        <button class="profile" style:--i={i} onclick={() => pick(p)}>
          <Avatar uuid={p.id} size={32} />
          <strong>{p.name}</strong>
          <span class="mono muted">{p.id.slice(0, 8)}…</span>
        </button>
      {/each}
    </div>
  {:else}
    <form
      class="form"
      onsubmit={(e) => {
        e.preventDefault();
        login();
      }}
    >
      <label>
        <span class="kicker">认证服务器</span>
        <input class="input" bind:value={server} placeholder="例如 littleskin.cn" disabled={phase === "busy"} />
      </label>
      <div class="hint">
        <span class="presets">
          {#each PRESETS as p}
            <button type="button" class="tag" class:accent={server.trim() === p.url} onclick={() => (server = p.url)}>
              {p.label}
            </button>
          {/each}
        </span>
        <span class="muted small status">
          {#if checking}正在检查…{:else if info}<span class="ok">✓ {info.name}</span>
            {#if info.registerUrl}· <button type="button" class="link" onclick={() => openUrl(info!.registerUrl!)}>注册账号</button>{/if}
          {:else if server.trim()}未能连接到这个服务器{/if}
        </span>
      </div>
      <label>
        <span class="kicker">账号（邮箱或用户名）</span>
        <input class="input" bind:value={username} autocomplete="username" disabled={phase === "busy"} />
      </label>
      <label>
        <span class="kicker">密码</span>
        <input class="input" type="password" bind:value={password} autocomplete="current-password" disabled={phase === "busy"} />
      </label>
      <p class="muted small">密码只用于这一次登录，不会被保存；之后使用服务器发放的令牌，保存在系统密钥库中。</p>
      {#if phase === "busy"}<ProgressBar indeterminate size="sm" />{/if}
      {#if error}
        <div class="err">
          <strong>{error.message}</strong>
          {#if error.hint}<p>{error.hint}</p>{/if}
        </div>
      {/if}
      <button type="submit" hidden aria-hidden="true"></button>
    </form>
  {/if}
  {#snippet actions()}
    <button class="btn" onclick={cancel}>取消</button>
    {#if phase !== "choose"}
      <button class="btn primary" disabled={phase === "busy" || !server.trim() || !username.trim() || !password} onclick={login}>
        登录
      </button>
    {/if}
  {/snippet}
</Modal>

<style>
  .form {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .hint {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    margin-top: -6px;
  }
  .presets {
    display: flex;
    gap: 6px;
  }
  .presets .tag {
    cursor: pointer;
    font: inherit;
    font-size: 11px;
    font-weight: 600;
    background: transparent;
    transition: border-color var(--t-fast), color var(--t-fast);
  }
  .presets .tag:hover {
    border-color: var(--ink-2);
  }
  .status {
    text-align: right;
  }
  .ok {
    color: var(--ok, var(--accent));
  }
  .small {
    font-size: 12px;
  }
  .lead {
    font-size: 13px;
    margin-bottom: 10px;
  }
  .profiles {
    display: flex;
    flex-direction: column;
    border-top: 1px solid var(--rule-strong);
  }
  .profile {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 6px;
    border: 0;
    border-bottom: 1px solid var(--rule);
    background: transparent;
    cursor: pointer;
    text-align: left;
    transition: background-color var(--t-fast);
  }
  .profile:hover {
    background: var(--accent-soft);
  }
  .profile strong {
    flex: 1;
    font-family: var(--serif);
    font-size: 15px;
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
