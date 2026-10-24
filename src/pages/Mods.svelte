<script lang="ts">
  import { app } from "../lib/state.svelte";
</script>

<div class="page-head">
  <div>
    <div class="kicker">MODS</div>
    <h1>Mod 下载</h1>
    <div class="sub">数据来自 Modrinth</div>
  </div>
</div>

<div class="notice">
  <div class="kicker">预告</div>
  <h2>Mod 搜索与前置依赖自动安装即将上线</h2>
  <p>
    下一阶段会加入 Fabric / Quilt / Forge / NeoForge 加载器安装，以及基于 Modrinth 的 Mod 搜索、版本筛选和前置依赖自动解析。
    现在可以先安装原版实例，并在实例目录的 mods 文件夹中手动放入 Mod。
  </p>
  <button class="btn" onclick={() => (app.page = "instances")}>查看实例</button>
</div>

<style>
  .notice {
    max-width: 620px;
    padding: 24px 0;
  }
  .notice h2 {
    font-size: 28px;
    margin: 8px 0 12px;
  }
  .notice p {
    font-family: var(--serif);
    color: var(--ink-2);
    margin-bottom: 18px;
    font-size: 15px;
  }
</style>
