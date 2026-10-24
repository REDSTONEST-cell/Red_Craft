<script lang="ts">
  // 像素头像：离线账号按 UUID 选择 Steve 或 Alex 的配色（与游戏默认皮肤规则一致）。
  let { uuid = "", size = 40, skin = null }: { uuid?: string; size?: number; skin?: string | null } = $props();
  let skinFailed = $state(false);

  // Java: UUID.hashCode() & 1，等价于对各 32 位分段异或后取最低位
  const alex = $derived.by(() => {
    const hex = uuid.replace(/-/g, "");
    if (hex.length !== 32) return false;
    let x = 0;
    for (let i = 0; i < 4; i++) x ^= parseInt(hex.slice(i * 8, i * 8 + 8), 16);
    return (x & 1) === 1;
  });

  const STEVE = [
    "HHHHHHHH",
    "HHHHHHHH",
    "HSSSSSSH",
    "SSSSSSSS",
    "SWESSEWS",
    "SSSNNSSS",
    "SSMSSMSS",
    "SSMMMMSS",
  ];
  const ALEX = [
    "OOOOOOOO",
    "OOOOOOOO",
    "OSSSSOOO",
    "SSSSSSOO",
    "SWGSSGWS",
    "SSSSSSSS",
    "SSSMMSSS",
    "SSSSSSSS",
  ];
  const colors: Record<string, string> = {
    H: "#3b2a1a",
    S: "#c8956b",
    W: "#ffffff",
    E: "#4a3fa0",
    N: "#9c6343",
    M: "#7a4a33",
    O: "#e0883a",
    G: "#3f8a4a",
  };
  const grid = $derived(alex ? ALEX : STEVE);
</script>

{#if skin && !skinFailed}
  <!-- 皮肤贴图 64×64：脸在 (8,8)，帽子层在 (40,8)；背景按 8 倍放大后偏移 -->
  <span class="avatar skin" style:width="{size}px" style:height="{size}px" role="img" aria-label="皮肤头像">
    <span class="layer" style:background-image="url({skin})" style:background-size="{size * 8}px auto" style:background-position="-{size}px -{size}px"></span>
    <span class="layer" style:background-image="url({skin})" style:background-size="{size * 8}px auto" style:background-position="-{size * 5}px -{size}px"></span>
    <img src={skin} alt="" hidden onerror={() => (skinFailed = true)} />
  </span>
{:else}
<svg width={size} height={size} viewBox="0 0 8 8" shape-rendering="crispEdges" class="avatar" aria-hidden="true">
  {#each grid as row, y}
    {#each row.split("") as c, x}
      <rect {x} {y} width="1" height="1" fill={colors[c]} />
    {/each}
  {/each}
</svg>
{/if}

<style>
  .skin {
    /* 内容区正好是 size，边框画在外面，保证贴图偏移准确 */
    box-sizing: content-box;
    position: relative;
    overflow: hidden;
    background: var(--paper-2);
  }
  .layer {
    position: absolute;
    inset: 0;
    image-rendering: pixelated;
    background-repeat: no-repeat;
  }
  .avatar {
    display: block;
    border: 1.5px solid var(--ink);
    flex: none;
  }
</style>
