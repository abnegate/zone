import { createContext, type ReactNode, useCallback, useContext, useMemo, useState } from 'react';

interface NavDrawerContextType {
  open: boolean;
  toggle: () => void;
  close: () => void;
}

const NavDrawerContext = createContext<NavDrawerContextType | null>(null);

/// The phone's nav drawer, or null for a bar rendered outside the app frame,
/// which has no drawer to open. The frame holds the state so the drawer lives
/// in the sidebar and its trigger lives in whichever page bar is on screen.
export function useNavDrawer(): NavDrawerContextType | null {
  return useContext(NavDrawerContext);
}

export function NavDrawerProvider({ children }: { children: ReactNode }) {
  const [open, setOpen] = useState(false);
  const toggle = useCallback(() => setOpen((current) => !current), []);
  const close = useCallback(() => setOpen(false), []);
  const value = useMemo(() => ({ open, toggle, close }), [open, toggle, close]);

  return <NavDrawerContext.Provider value={value}>{children}</NavDrawerContext.Provider>;
}
