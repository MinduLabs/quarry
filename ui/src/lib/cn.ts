import { type ClassValue, clsx } from "clsx";

/** Join class names; falsy values are dropped. */
export function cn(...classes: ClassValue[]): string {
  return clsx(classes);
}
