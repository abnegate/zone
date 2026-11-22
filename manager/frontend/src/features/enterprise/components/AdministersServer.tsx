import { createContext, useContext } from 'react';

const AdministersServer = createContext(false);

export const AdministersServerProvider = AdministersServer.Provider;

export function useAdministersServer(): boolean {
  return useContext(AdministersServer);
}
