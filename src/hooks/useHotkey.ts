import { useEffect } from "react";

type KeyMap = Record<string, () => void>;

/**
 * Bind keyboard shortcuts while `enabled` is true. Cleans up on unmount or
 * when `enabled` becomes false.
 *
 * - Keys are matched against `event.key` (e.g. "1", "2", "Escape",
 *   "ArrowLeft", "ArrowRight").
 * - Handlers are skipped when the active element is an input / textarea /
 *   select / contenteditable — prevents hotkeys from firing while the user
 *   is typing (defensive; the overlay has no text inputs, but still correct).
 * - `map` should be stable across renders (define outside the component or
 *   wrap in useMemo) to avoid spurious effect re-runs.
 */
export function useHotkey(map: KeyMap, enabled: boolean): void {
  useEffect(() => {
    if (!enabled) return;

    function handleKeyDown(e: KeyboardEvent): void {
      // Skip when focus is inside a text input.
      const target = e.target as HTMLElement | null;
      if (
        target instanceof HTMLInputElement ||
        target instanceof HTMLTextAreaElement ||
        target instanceof HTMLSelectElement ||
        target?.isContentEditable
      ) {
        return;
      }

      const handler = map[e.key];
      if (handler) {
        e.preventDefault();
        handler();
      }
    }

    window.addEventListener("keydown", handleKeyDown);
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, [map, enabled]);
}
