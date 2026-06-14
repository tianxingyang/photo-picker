import { create } from "zustand";

import {
  closeProject,
  createProject,
  deleteProject,
  listProjects,
  openProject,
  type ProjectSummary,
} from "../api/projectsApi";
import { clearDisplayCache } from "../hooks/useDisplaySrc";
import { useCompareStore } from "./compareStore";
import { useGroupsStore } from "./groupsStore";
import { usePhotosStore } from "./photosStore";

type ProjectsState = {
  projects: ProjectSummary[];
  /** UUID of the open project; null => show the landing page. */
  currentProjectId: string | null;
  loaded: boolean;
  loadProjects: () => Promise<void>;
  open: (id: string) => Promise<void>;
  close: () => Promise<void>;
  create: (name: string) => Promise<ProjectSummary>;
  remove: (id: string) => Promise<void>;
};

// why: switching projects must not leak project A's photos, grid, compare
// overlay, or cached HEIC transcodes into project B. Clear every photo-scoped
// store + the display-src cache whenever the open project changes.
function resetPhotoScopedStores(): void {
  usePhotosStore.getState().clear();
  useGroupsStore.getState().clear();
  useCompareStore.getState().close();
  clearDisplayCache();
}

export const useProjectsStore = create<ProjectsState>((set) => ({
  projects: [],
  currentProjectId: null,
  loaded: false,

  loadProjects: async () => {
    const projects = await listProjects();
    set({ projects, loaded: true });
  },

  // Open a project: tell the backend (sets the process-wide current project),
  // clear any prior project's cached state, then mark it current so App routes
  // to the grid. The grid hydrates itself via groupsStore.load().
  open: async (id) => {
    await openProject(id);
    resetPhotoScopedStores();
    set({ currentProjectId: id });
  },

  // Return to the landing page. Clear scoped state so a re-open starts clean;
  // the landing view reloads the list on mount (fresh photo counts / order).
  close: async () => {
    await closeProject();
    resetPhotoScopedStores();
    set({ currentProjectId: null });
  },

  create: async (name) => {
    const project = await createProject(name);
    // Prepend optimistically; loadProjects() on next landing re-sorts canonically.
    set((s) => ({ projects: [project, ...s.projects] }));
    return project;
  },

  remove: async (id) => {
    await deleteProject(id);
    set((s) => ({ projects: s.projects.filter((p) => p.id !== id) }));
  },
}));
