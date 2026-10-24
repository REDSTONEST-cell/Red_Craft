<script lang="ts">
  // 像素头像：离线账号按 UUID 选择 Steve 或 Alex 的配色（与游戏默认皮肤规则一致）。
  let { uuid = "", size = 40 }: { uuid?: string; size?: number } = $props();

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

<svg width={size} height={size} viewBox="0 0 8 8" shape-rendering="crispEdges" class="avatar" aria-hidden="true">
  {#each grid as row, y}
    {#each row.split("") as c, x}
      <rect {x} {y} width="1" height="1" fill={colors[c]} />
    {/each}
  {/each}
</svg>

<style>
  .avatar {
    display: block;
    border: 1.5px solid var(--ink);
    flex: none;
  }
</style>
