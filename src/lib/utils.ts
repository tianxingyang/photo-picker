import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

// shadcn/ui convention: merge conditional class lists and dedupe conflicting
// Tailwind utilities (last wins). Used by every styled component.
export function cn(...inputs: ClassValue[]): string {
  return twMerge(clsx(inputs));
}
