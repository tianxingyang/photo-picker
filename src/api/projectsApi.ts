import { invoke } from "@tauri-apps/api/core";

// Raw row from Rust `create_project` / `list_projects` (serde camelCase).
// Validated at the boundary because Rust↔TS type sharing is still OPEN (no
// codegen yet).
export type ProjectSummary = {
  id: string; // UUID v4
  name: string;
  createdAt: string;
  lastOpenedAt: string | null;
  photoCount: number;
};

function isProjectSummary(v: unknown): v is ProjectSummary {
  if (!v || typeof v !== "object") return false;
  const o = v as Record<string, unknown>;
  return (
    typeof o.id === "string" &&
    typeof o.name === "string" &&
    typeof o.createdAt === "string" &&
    (o.lastOpenedAt === null || typeof o.lastOpenedAt === "string") &&
    typeof o.photoCount === "number"
  );
}

export async function createProject(name: string): Promise<ProjectSummary> {
  const raw = await invoke<unknown>("create_project", { name });
  if (!isProjectSummary(raw)) {
    throw new Error("create_project returned an unexpected shape");
  }
  return raw;
}

export async function listProjects(): Promise<ProjectSummary[]> {
  const raw = await invoke<unknown>("list_projects");
  if (!Array.isArray(raw) || !raw.every(isProjectSummary)) {
    throw new Error("list_projects returned an unexpected shape");
  }
  return raw;
}

// Tauri v2: JS camelCase `projectId` ↔ Rust snake_case `project_id`.
export async function openProject(projectId: string): Promise<void> {
  await invoke("open_project", { projectId });
}

export async function closeProject(): Promise<void> {
  await invoke("close_project");
}

export async function deleteProject(projectId: string): Promise<void> {
  await invoke("delete_project", { projectId });
}
