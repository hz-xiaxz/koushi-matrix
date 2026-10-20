import { createContext, useContext } from "react";

/**
 * #1557: `t()` reads a module-level active locale that `App` sets during its
 * render. A component wrapped in `memo()` re-renders only when its props change,
 * so after a language switch it would keep the strings of its last render.
 * `App` provides this key (catalog locale plus pseudo mode); a memoised
 * component that renders catalog text subscribes with `useActiveLocaleKey()`,
 * and the context change re-renders it in place, without a remount.
 */
export const ActiveLocaleContext = createContext<string>("en:none");

export function activeLocaleKey(catalogLocale: string, pseudoLocale: string): string {
  return `${catalogLocale}:${pseudoLocale}`;
}

export function useActiveLocaleKey(): string {
  return useContext(ActiveLocaleContext);
}
