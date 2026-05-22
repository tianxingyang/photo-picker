import { invoke } from "@tauri-apps/api/core";

export async function echo(text: string): Promise<string> {
  return invoke<string>("echo_via_sidecar", { text });
}
