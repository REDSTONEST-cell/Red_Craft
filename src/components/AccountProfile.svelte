<script lang="ts">
  import { onMount } from "svelte";
  import { open as chooseFile, save } from "@tauri-apps/plugin-dialog";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { api, type Account, type AccountProfile } from "../lib/api";
  import { refreshAccounts, showError, toast } from "../lib/state.svelte";
  import Modal from "./Modal.svelte";
  let { account, mode, onclose }: { account: Account; mode: "skin" | "info"; onclose: () => void } = $props();
  let open = $state(true);
  let busy = $state(false);
  let profile = $state<AccountProfile | null>(null);
  let variant = $state("classic");
  let file = $state("");
  let cape = $state("");
  let error = $state("");
  async function load() {
    profile = await api.accountProfile(account.id);
    cape = profile.capes.find(c => c.state === "ACTIVE")?.id ?? "";
    await refreshAccounts();
  }
  async function run(action: () => Promise<void>, success = "") {
    if (busy) return;
    busy = true; error = "";
    try { await action(); if (success) toast("ok", success); }
    catch (e) { error = "操作失败，请检查网络或登录状态后重试。"; showError(e, "账号操作失败"); }
    finally { busy = false; }
  }
  onMount(() => { if (mode === "skin") void run(load); });
  async function choose() {
    const path = await chooseFile({ multiple: false, filters: [{ name: "PNG 皮肤", extensions: ["png"] }] });
    if (typeof path === "string") file = path;
  }
  async function saveSkin() {
    const path = await save({ defaultPath: `${account.name}.png`, filters: [{ name: "PNG 皮肤", extensions: ["png"] }] });
    if (path) { await api.accountSaveSkin(account.id, path); toast("ok", "皮肤文件已保存"); }
  }
</script>
<Modal bind:open title={mode === "skin" ? "皮肤与披风" : "修改信息"} kicker={account.name} {onclose} width={520}>
  {#if mode === "info"}
    <p class="muted">在官方网页中修改信息，请确认浏览器登录的是 {account.name} 对应的微软账号。修改玩家名后可在账号列表刷新。</p>
    <div class="actions">
      <button class="btn" onclick={() => run(() => openUrl("https://account.live.com/password/Change"))}>修改密码</button>
      <button class="btn" onclick={() => run(() => openUrl("https://www.minecraft.net/zh-hans/msaprofile/mygames/editprofile"))}>修改玩家名</button>
    </div>
  {:else}
    <h3>皮肤</h3>
    <p class="muted small">选择 64×64 或 64×32 的 PNG 文件，并选择手臂模型。</p>
    <div class="actions">
      <button class="btn" disabled={busy} onclick={() => run(choose)}>选择皮肤文件</button>
      <select class="input" aria-label="皮肤模型" bind:value={variant} disabled={busy}><option value="classic">经典 · 宽手臂</option><option value="slim">纤细 · 窄手臂</option></select>
    </div>
    {#if file}<p class="filename small">{file}</p>{/if}
    <div class="actions">
      <button class="btn primary" disabled={busy || !file} onclick={() => run(async () => { await api.accountUploadSkin(account.id, file, variant); await load(); }, "皮肤已修改")}>上传皮肤</button>
      <button class="btn" disabled={busy || !profile?.skins.length} onclick={() => run(saveSkin)}>保存皮肤文件</button>
    </div>
    <h3>披风</h3>
    {#if profile}
      {#if !profile.capes.length}<p class="muted small">此账号暂时没有可用的披风。</p>{/if}
      <div class="actions">
        <select class="input" aria-label="披风" bind:value={cape} disabled={busy}>
          <option value="">不使用披风</option>
          {#each profile.capes as c}<option value={c.id}>{c.alias || c.id}{c.state === "ACTIVE" ? "（使用中）" : ""}</option>{/each}
        </select>
        <button class="btn primary" disabled={busy} onclick={() => run(async () => { await api.accountSetCape(account.id, cape || null); await load(); }, "披风已修改")}>应用披风</button>
      </div>
    {/if}
    <div class="actions">
      <button class="btn" disabled={busy} onclick={() => run(load, "皮肤与披风列表已刷新")}>刷新皮肤与披风</button>
      <button class="btn ghost" onclick={() => run(() => openUrl("https://www.minecraft.net/zh-hans/redeem"))}>使用 CDKEY 兑换奖励</button>
    </div>
  {/if}
  {#if busy}<p class="muted" role="status">正在处理，请稍候…</p>{/if}
  {#if error}<p role="alert">{error}</p>{/if}
</Modal>
<style>
  .actions { display: flex; flex-wrap: wrap; gap: 8px; margin: 12px 0; }
  .actions select { flex: 1; min-width: 150px; }
  .filename { overflow-wrap: anywhere; }
  h3 { margin-top: 18px; }
</style>
