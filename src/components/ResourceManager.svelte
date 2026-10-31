<script lang="ts">
  // 资源管理：实例中的 Mod、材质包、光影包与存档。支持选择文件导入，或把文件拖进窗口导入到当前类别。
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { api, type FileKind } from "../lib/api";
  import { app, showError, toast } from "../lib/state.svelte";
  import InstalledMods from "./InstalledMods.svelte";
  import InstalledFiles from "./InstalledFiles.svelte";

  let { instance, kind }: { instance: string; kind: FileKind } = $props();

  const LABEL: Record<FileKind, string> = { mod: "Mod", resourcepack: "材质包", shader: "光影包", save: "存档" };
  let reload = $state(0);

  async function importPaths(paths: string[]) {
    try {
      const names = await api.filesImport(instance, kind, paths);
      toast("ok", names.length === 1 ? `已导入「${names[0]}」` : `已导入 ${names.length} 个${LABEL[kind]}`);
    } catch (e) {
      showError(e, `导入${LABEL[kind]}失败`);
    }
    reload++;
  }

  async function pick() {
    const filters: Record<FileKind, { name: string; extensions: string[] }[]> = {
      mod: [{ name: "Mod", extensions: ["jar"] }],
      resourcepack: [{ name: "材质包", extensions: ["zip"] }],
      shader: [{ name: "光影包", extensions: ["zip"] }],
      save: [],
    };
    try {
      // 存档选择文件夹；存档压缩包可以直接拖进窗口
      const picked = await openDialog({
        multiple: true,
        directory: kind === "save",
        title: `导入${LABEL[kind]}`,
        filters: filters[kind],
      });
      const paths = picked === null ? [] : Array.isArray(picked) ? picked : [picked];
      if (paths.length) await importPaths(paths);
    } catch (e) {
      showError(e);
    }
  }

  // 打开资源管理时，拖进窗口的文件导入到当前类别（而不是当作整合包）
  $effect(() => {
    const k = kind;
    app.dropTarget = { label: LABEL[k], drop: (paths) => void importPaths(paths) };
    return () => {
      app.dropTarget = null;
    };
  });
</script>

{#if kind === "mod"}
  <InstalledMods {instance} {reload} onimport={pick} />
{:else}
  <InstalledFiles {instance} {kind} {reload} onimport={pick} />
{/if}
