<script lang="ts">
  import { api } from "../lib/api";
  import { app, refreshAccounts, showError, toast } from "../lib/state.svelte";
  import Avatar from "../components/Avatar.svelte";
  import Icon from "../components/Icon.svelte";
  import MsLogin from "../components/MsLogin.svelte";

  let msOpen = $state(false);
  let refreshing = $state<string | null>(null);

  async function refreshMs(id: string, offline = false) {
    refreshing = id;
    try {
      if (offline) {
        const a = await api.refreshOfflineSkin(id);
        await refreshAccounts();
        toast("ok", a.skin ? `已使用正版账号 ${a.name} 的皮肤` : `没有找到名为 ${a.name} 的正版账号，使用默认头像`);
        return;
      }
      const a = await api.msRefreshAccount(id);
      await refreshAccounts();
      toast("ok", `账号 ${a.name} 登录状态正常`);
    } catch (e) {
      showError(e, "刷新失败");
    } finally {
      refreshing = null;
    }
  }

  let name = $state("");
  let busy = $state(false);

  async function addOffline() {
    busy = true;
    try {
      const r = await api.addOfflineAccount(name);
      name = "";
      await refreshAccounts();
      toast(r.warning ? "info" : "ok", `已添加离线账号 ${r.account.name}`, r.warning ?? undefined);
    } catch (e) {
      showError(e, "无法添加账号");
    } finally {
      busy = false;
    }
  }

  async function select(id: string) {
    app.selectedAccount = id;
    await api.selectAccount(id).catch((e) => showError(e));
  }

  async function remove(id: string) {
    try {
      await api.removeAccount(id);
      await refreshAccounts();
    } catch (e) {
      showError(e, "删除失败");
    }
  }

  const kindLabel = (t: string) => (t === "offline" ? "离线" : t === "microsoft" ? "微软正版" : "外置登录");
</script>

<div class="page-head">
  <div>
    <div class="kicker">ACCOUNTS</div>
    <h1>账号</h1>
    <div class="sub">可保存多个账号，点击即可切换</div>
  </div>
</div>

<div class="grid">
  <section>
    <h3 class="sec">已保存的账号</h3>
    {#if app.accounts.length === 0}
      <div class="empty"><h3>还没有账号</h3><p>在右侧添加一个离线账号即可开始游戏。</p></div>
    {:else}
      <div class="list stagger">
        {#each app.accounts as a, i (a.id)}
          <div class="acc" class:active={app.selectedAccount === a.id} style:--i={i}>
            <button class="pick" onclick={() => select(a.id)}>
              <Avatar uuid={a.uuid} skin={a.skin} size={42} />
              <div class="info">
                <strong>{a.name}</strong>
                <span class="muted">{kindLabel(a.kind.type)} · <span class="mono">{a.uuid.slice(0, 8)}…</span></span>
              </div>
              {#if app.selectedAccount === a.id}<span class="tag accent"><Icon name="check" size={10} /> 使用中</span>{/if}
            </button>
            {#if a.kind.type === "microsoft" || a.kind.type === "offline"}
              <button
                class="btn ghost icon sm"
                title={a.kind.type === "offline" ? "重新获取同名正版皮肤" : "刷新登录状态"}
                disabled={refreshing === a.id}
                onclick={() => refreshMs(a.id, a.kind.type === "offline")}
              >
                <span class:spin={refreshing === a.id}><Icon name="refresh" size={14} /></span>
              </button>
            {/if}
            <button class="btn ghost icon sm" title="删除账号" onclick={() => remove(a.id)}><Icon name="trash" size={14} /></button>
          </div>
        {/each}
      </div>
    {/if}
  </section>

  <aside>
    <h3 class="sec">添加账号</h3>
    <div class="box">
      <div class="kicker">离线登录</div>
      <p class="muted small">无需联网，适合单人游戏与离线服务器。UUID 与服务端离线模式生成规则一致。</p>
      <form
        class="add"
        onsubmit={(e) => {
          e.preventDefault();
          addOffline();
        }}
      >
        <input class="input" placeholder="玩家名，例如 Steve" bind:value={name} maxlength="16" />
        <button class="btn primary" disabled={busy || !name.trim()}>添加</button>
      </form>
    </div>
    <div class="box">
      <div class="kicker">微软正版登录</div>
      <p class="muted small">在浏览器中登录购买了 Minecraft 的微软账号，启动器不会接触你的密码。令牌保存在系统密钥库中。</p>
      <button class="btn primary" onclick={() => (msOpen = true)}>使用微软账号登录</button>
    </div>
    <div class="box disabled">
      <div class="kicker">外置登录</div>
      <p class="muted small">即将推出：支持 LittleSkin 等 authlib-injector 认证服务器。</p>
    </div>
  </aside>
</div>

<MsLogin bind:open={msOpen} />

<style>
  .grid {
    display: grid;
    grid-template-columns: minmax(0, 1.4fr) minmax(260px, 1fr);
    gap: 28px;
  }
  .sec {
    font-size: 15px;
    padding-bottom: 6px;
    border-bottom: 1px solid var(--rule-strong);
    margin-bottom: 8px;
  }
  .acc {
    display: flex;
    align-items: center;
    gap: 8px;
    border-bottom: 1px solid var(--rule);
    padding-right: 6px;
  }
  .pick {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 12px;
    border: 0;
    background: transparent;
    padding: 12px 6px;
    cursor: pointer;
    text-align: left;
    transition: transform var(--t-fast) var(--ease);
  }
  .pick:active {
    transform: scale(0.99);
  }
  .acc:hover {
    background: var(--accent-soft);
  }
  .info {
    flex: 1;
    min-width: 0;
  }
  .info strong {
    display: block;
    font-family: var(--serif);
    font-size: 16px;
  }
  .info span {
    font-size: 12px;
  }
  aside {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  aside .sec {
    margin-bottom: 0;
  }
  .box {
    padding: 14px;
    border: 1px solid var(--rule);
    background: var(--card);
  }
  .box.disabled {
    opacity: 0.6;
  }
  .small {
    font-size: 12px;
    margin: 4px 0 10px;
  }
  .add {
    display: flex;
    gap: 8px;
  }
  .add .input {
    flex: 1;
  }
</style>
