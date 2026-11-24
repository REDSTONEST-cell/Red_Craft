import { backgroundApi, defaults, type Appearance, type BackgroundLibrary } from "./background";

export const background = $state({ saved: defaults(), preview: null as Appearance | null, error: "", ready: false, retry: 0,
  /** 当前显示的背景画面的平均亮度（用于自动选择背景上的文字颜色） */
  luminance: null as number | null });
let savedRevision = 0;
export async function loadBackground(): Promise<BackgroundLibrary> {
  const revision = ++savedRevision;
  const library = await backgroundApi.list();
  if (revision === savedRevision) background.saved = library.appearance;
  return library;
}
// Background persistence never calls the launcher's whole-settings save API.
// Only the backend's successfully persisted result becomes the active selection.
export async function applyBackground(value: Appearance) {
  const submittedPreview = background.preview;
  const saved = await backgroundApi.apply({ ...value });
  savedRevision++;
  background.saved = saved;
  if (background.preview === submittedPreview) {
    background.preview = null;
    background.error = "";
  }
}
