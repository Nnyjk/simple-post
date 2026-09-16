import { clsx, type ClassValue } from 'clsx';
import { twMerge } from 'tailwind-merge';

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export function formatBytes(bytes: number): string {
  if (!bytes) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB'];
  let i = 0;
  let n = bytes;
  while (n >= 1024 && i < units.length - 1) {
    n /= 1024;
    i++;
  }
  return `${n.toFixed(n < 10 && i > 0 ? 1 : 0)} ${units[i]}`;
}

export function formatDuration(ms: number): string {
  if (ms < 1) return '< 1 ms';
  if (ms < 1000) return `${Math.round(ms)} ms`;
  return `${(ms / 1000).toFixed(2)} s`;
}

export function methodColorVar(method: string): string {
  switch (method.toUpperCase()) {
    case 'GET':
      return 'hsl(142 71% 45%)';
    // POST and PUT are intentionally swapped from a typical REST-coloring
    // convention: blue is reserved for "create" (POST) elsewhere in the
    // product, so POST takes the warmer orange and PUT takes the cooler
    // blue. This way the two create/update verbs don't blend visually
    // when they sit next to each other in the tree.
    case 'POST':
      return 'hsl(38 92% 50%)';
    case 'PUT':
      return 'hsl(217 91% 60%)';
    case 'PATCH':
      return 'hsl(280 65% 60%)';
    case 'DELETE':
      return 'hsl(0 72% 51%)';
    default:
      return 'hsl(220 14% 46%)';
  }
}

export function statusColorVar(status: number): string {
  if (status >= 500) return 'hsl(0 72% 51%)';
  if (status >= 400) return 'hsl(38 92% 50%)';
  if (status >= 300) return 'hsl(217 91% 60%)';
  if (status >= 200) return 'hsl(142 71% 45%)';
  return 'hsl(220 14% 46%)';
}

export function uid(): string {
  return Math.random().toString(36).slice(2, 10) + Date.now().toString(36);
}
