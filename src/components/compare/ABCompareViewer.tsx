import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { useGroupsStore } from "../../store/groupsStore";
import { useCompareStore } from "../../store/compareStore";
import { useHotkey } from "../../hooks/useHotkey";
import { ComparePane } from "./ComparePane";
import { CompareToolbar } from "./CompareToolbar";
import { CompareKeepBar } from "./CompareKeepBar";
import { CompareFilmstrip } from "./CompareFilmstrip";
import type { View } from "./ComparePane";

const MIN_SCALE = 1;
const MAX_SCALE = 8;
const ZOOM_STEP = 0.25;

function clamp(v: number, lo: number, hi: number) {
  return Math.max(lo, Math.min(hi, v));
}

const INITIAL_VIEW: View = { scale: 1, tx: 0, ty: 0 };

export function ABCompareViewer() {
  const open = useCompareStore((s) => s.open);
  const aId = useCompareStore((s) => s.aId);
  const bId = useCompareStore((s) => s.bId);
  const memberIds = useCompareStore((s) => s.memberIds);
  const close = useCompareStore((s) => s.close);
  const stepB = useCompareStore((s) => s.stepB);
  const setStatus = useGroupsStore((s) => s.setStatus);

  // Shared view state — single transform drives both panes (the lock).
  const [view, setView] = useState<View>(INITIAL_VIEW);

  // Reset view when pair changes (swap, setB, open new photo).
  const prevARef = useRef(aId);
  const prevBRef = useRef(bId);
  useEffect(() => {
    if (prevARef.current !== aId || prevBRef.current !== bId) {
      setView(INITIAL_VIEW);
    }
    prevARef.current = aId;
    prevBRef.current = bId;
  }, [aId, bId]);

  // Also reset view when overlay opens.
  useEffect(() => {
    if (open) setView(INITIAL_VIEW);
  }, [open]);

  // Focus management: remember what triggered the open so we can restore focus on close.
  const triggerRef = useRef<Element | null>(null);
  const overlayRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (open) {
      triggerRef.current = document.activeElement;
      // Move focus into the overlay (close button).
      const firstFocusable = overlayRef.current?.querySelector<HTMLElement>(
        "button, [tabindex]:not([tabindex='-1'])",
      );
      firstFocusable?.focus();
    } else {
      // Restore focus to the trigger element when closing.
      if (triggerRef.current instanceof HTMLElement) {
        triggerRef.current.focus();
      }
    }
  }, [open]);

  // Trap focus inside overlay: Tab/Shift+Tab must not escape to the grid below.
  useEffect(() => {
    if (!open) return;
    const el = overlayRef.current;
    if (!el) return;

    function trapTab(e: KeyboardEvent) {
      if (e.key !== "Tab" || !el) return;
      const focusable = Array.from(
        el.querySelectorAll<HTMLElement>("button:not([disabled]), [tabindex]:not([tabindex='-1'])"),
      ).filter((n) => !n.closest("[aria-hidden='true']"));
      if (focusable.length === 0) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (e.shiftKey) {
        if (document.activeElement === first) {
          e.preventDefault();
          last?.focus();
        }
      } else {
        if (document.activeElement === last) {
          e.preventDefault();
          first?.focus();
        }
      }
    }

    el.addEventListener("keydown", trapTab);
    return () => el.removeEventListener("keydown", trapTab);
  }, [open]);

  // Zoom helper.
  const zoom = useCallback((delta: number, cx = 0, cy = 0) => {
    setView((prev) => {
      const newScale = clamp(prev.scale + delta, MIN_SCALE, MAX_SCALE);
      if (newScale === prev.scale) return prev;
      // Anchor zoom to the focal point so the image pivots under the cursor.
      const ratio = newScale / prev.scale;
      return {
        scale: newScale,
        tx: cx - (cx - prev.tx) * ratio,
        ty: cy - (cy - prev.ty) * ratio,
      };
    });
  }, []);

  // Hotkeys.
  const hotkeyMap = useMemo(
    () => ({
      "1": () => {
        if (aId !== null) {
          setStatus(aId, "keep").catch((e) => {
            if (import.meta.env.DEV) console.debug("setStatus failed", e);
          });
        }
      },
      "2": () => {
        if (bId !== null) {
          setStatus(bId, "keep").catch((e) => {
            if (import.meta.env.DEV) console.debug("setStatus failed", e);
          });
        }
      },
      ArrowLeft: () => stepB(-1),
      ArrowRight: () => stepB(1),
      Escape: close,
    }),
    [aId, bId, setStatus, stepB, close],
  );

  useHotkey(hotkeyMap, open);

  // Drag-to-pan state (pointer events, not mouse — handles touch as well).
  const dragRef = useRef<{
    startX: number;
    startY: number;
    startTx: number;
    startTy: number;
  } | null>(null);

  function handlePointerDown(e: React.PointerEvent<HTMLDivElement>) {
    if (e.button !== 0) return; // left button / touch only
    (e.currentTarget as HTMLDivElement).setPointerCapture(e.pointerId);
    dragRef.current = { startX: e.clientX, startY: e.clientY, startTx: view.tx, startTy: view.ty };
  }

  function handlePointerMove(e: React.PointerEvent<HTMLDivElement>) {
    // why: snapshot the drag base into a local BEFORE setView. The updater
    // closure runs later (React 18 batches updates via the scheduler); by then
    // a pointerup may have set dragRef.current = null. Dereferencing the ref
    // inside the closure (dragRef.current!.startTx) then throws "null.startTx"
    // during the render phase, which unmounts the whole tree → blank screen.
    const drag = dragRef.current;
    if (!drag) return;
    const dx = e.clientX - drag.startX;
    const dy = e.clientY - drag.startY;
    setView((prev) => ({
      ...prev,
      tx: drag.startTx + dx,
      ty: drag.startTy + dy,
    }));
  }

  function handlePointerUp(e: React.PointerEvent<HTMLDivElement>) {
    (e.currentTarget as HTMLDivElement).releasePointerCapture(e.pointerId);
    dragRef.current = null;
  }

  function handleWheel(e: React.WheelEvent<HTMLDivElement>) {
    e.stopPropagation();
    // Get pointer position relative to the pane container.
    const rect = (e.currentTarget as HTMLDivElement).getBoundingClientRect();
    const cx = e.clientX - rect.left - rect.width / 2;
    const cy = e.clientY - rect.top - rect.height / 2;
    const delta = e.deltaY < 0 ? ZOOM_STEP : -ZOOM_STEP;
    zoom(delta, cx, cy);
  }

  // Derive toolbar label.
  const aPhoto = aId !== null ? useGroupsStore.getState().byId[aId] : undefined;
  const totalCount = memberIds.length;
  const aIdx = aId !== null ? memberIds.indexOf(aId) + 1 : 0;
  const toolbarLabel = aPhoto
    ? `${aIdx} / ${totalCount} · ${aPhoto.name}`
    : `${aIdx} / ${totalCount}`;

  if (!open) return null;

  // Reduced-motion: skip scale/translate on open, keep only opacity.
  return (
    <div
      ref={overlayRef}
      className="fixed inset-0 z-50 flex flex-col motion-safe:animate-[fadeIn_150ms_ease-out]"
      role="dialog"
      aria-modal="true"
      aria-label="A/B 对比"
      // Prevent clicks from propagating to the grid below.
      onClick={(e) => e.stopPropagation()}
    >
      {/* Toolbar */}
      <CompareToolbar
        label={toolbarLabel}
        scale={view.scale}
        onZoomIn={() => zoom(ZOOM_STEP)}
        onZoomOut={() => zoom(-ZOOM_STEP)}
        onZoomReset={() => setView(INITIAL_VIEW)}
        onClose={close}
      />

      {/* Photo panes */}
      <div className="flex min-h-0 flex-1">
        <ComparePane
          photoId={aId}
          view={view}
          side="left"
          onWheel={handleWheel}
          onPointerDown={handlePointerDown}
          onPointerMove={handlePointerMove}
          onPointerUp={handlePointerUp}
        />
        {/* 1px divider between panes */}
        <div className="w-px shrink-0 bg-border" aria-hidden="true" />
        <ComparePane
          photoId={bId}
          view={view}
          side="right"
          onWheel={handleWheel}
          onPointerDown={handlePointerDown}
          onPointerMove={handlePointerMove}
          onPointerUp={handlePointerUp}
        />
      </div>

      {/* Keep bar */}
      <CompareKeepBar aId={aId} bId={bId} />

      {/* Filmstrip */}
      <CompareFilmstrip />
    </div>
  );
}
