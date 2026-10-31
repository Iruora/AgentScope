import { useCallback, useEffect, useState } from "react";

export type Theme = "light" | "dark";

const THEME_KEY = "agentscope.theme";

/** Lets every `useTheme` user follow a toggle made by another (topbar, command palette). */
const THEME_EVENT = "agentscope:theme";

function stored(): Theme | null {
  try {
    const value = localStorage.getItem(THEME_KEY);
    return value === "light" || value === "dark" ? value : null;
  } catch {
    return null;
  }
}

function system(): Theme {
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

/** Sets the theme before the first render, so the window never flashes the wrong one. */
export function applyInitialTheme() {
  document.documentElement.dataset.theme = stored() ?? system();
}

/** The theme picked in the topbar, else the system's (followed live until one is picked). */
export function useTheme(): [Theme, () => void] {
  const [theme, setTheme] = useState<Theme>(() => stored() ?? system());

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);

  useEffect(() => {
    const query = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => {
      if (!stored()) setTheme(system());
    };
    const onToggle = (e: Event) => setTheme((e as CustomEvent<Theme>).detail);
    query.addEventListener("change", onChange);
    window.addEventListener(THEME_EVENT, onToggle);
    return () => {
      query.removeEventListener("change", onChange);
      window.removeEventListener(THEME_EVENT, onToggle);
    };
  }, []);

  const toggle = useCallback(() => {
    const next: Theme = document.documentElement.dataset.theme === "dark" ? "light" : "dark";
    try {
      localStorage.setItem(THEME_KEY, next);
    } catch {
      // The choice just won't be remembered.
    }
    window.dispatchEvent(new CustomEvent<Theme>(THEME_EVENT, { detail: next }));
  }, []);

  return [theme, toggle];
}
