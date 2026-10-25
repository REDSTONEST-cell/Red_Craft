import { backgroundApi, defaults, type Appearance, type BackgroundLibrary } from "./background";

export const background = $state({ saved: defaults(), preview: null as Appearance | null, error: "", ready: false, retry: 0 });
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
