<script lang="ts">
  import Background from "../src/components/BackgroundLayer.svelte";
  import BackgroundPicker from "../src/components/BackgroundPicker.svelte";
  import Toasts from "../src/components/Toasts.svelte";
  let dark = $state(false);
  let showPicker = $state(true);
  $effect(() => { document.documentElement.dataset.theme = dark ? "dark" : "light"; });
</script>
<Background />
<div class="test-shell">
  <div class="content">
    <div class="page-head"><div><div class="kicker">FRONTEND TEST FIXTURE</div><h1>外观与背景</h1><p>模拟 IPC · 不代表 WebView2 实机验收</p></div><button class="btn" onclick={() => dark = !dark}>切换浅色 / 深色</button></div>
    <button class="btn sm" onclick={() => showPicker = !showPicker}>{showPicker ? "卸载背景面板" : "挂载背景面板"}</button>
    {#if showPicker}<BackgroundPicker />{/if}
  </div>
</div>
<Toasts />
<style>
  .test-shell { position: relative; z-index: 2; height: 100%; padding: 22px; }
  .content { position: relative; height: 100%; overflow: auto; padding: 22px; border: 1px solid var(--rule); }
</style>
