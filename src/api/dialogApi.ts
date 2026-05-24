import { open } from "@tauri-apps/plugin-dialog";

// Native folder picker. Returns the chosen absolute path, or null if cancelled.
export async function pickFolder(): Promise<string | null> {
  const selected = await open({ directory: true, multiple: false });
  // why: open() is typed string | string[] | null; directory + !multiple
  // yields string | null, but narrow defensively rather than assert.
  return typeof selected === "string" ? selected : null;
}
