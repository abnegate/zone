import { type ReactNode, createContext, useCallback, useContext, useEffect, useState } from 'react';

interface AuthContextType {
  apiKey: string | null;
  isAuthenticated: boolean;
  login: (key: string) => Promise<boolean>;
  logout: () => void;
}

const AuthContext = createContext<AuthContextType | undefined>(undefined);

const API_KEY_STORAGE_KEY = 'manager_api_key';

export function AuthProvider({ children }: { children: ReactNode }) {
  const [apiKey, setApiKey] = useState<string | null>(() => {
    return localStorage.getItem(API_KEY_STORAGE_KEY);
  });

  const isAuthenticated = apiKey !== null;

  const login = useCallback(async (key: string): Promise<boolean> => {
    try {
      const response = await fetch('/api/models', {
        headers: {
          Authorization: `Bearer ${key}`,
        },
      });

      if (response.ok) {
        setApiKey(key);
        localStorage.setItem(API_KEY_STORAGE_KEY, key);
        return true;
      }
      return false;
    } catch {
      return false;
    }
  }, []);

  const logout = useCallback(() => {
    setApiKey(null);
    localStorage.removeItem(API_KEY_STORAGE_KEY);
  }, []);

  // Verify stored key on mount only
  useEffect(() => {
    const storedKey = localStorage.getItem(API_KEY_STORAGE_KEY);
    if (storedKey) {
      fetch('/api/models', {
        headers: {
          Authorization: `Bearer ${storedKey}`,
        },
      })
        .then((response) => {
          if (!response.ok) {
            setApiKey(null);
            localStorage.removeItem(API_KEY_STORAGE_KEY);
          }
        })
        .catch(() => {
          setApiKey(null);
          localStorage.removeItem(API_KEY_STORAGE_KEY);
        });
    }
  }, []);

  return (
    <AuthContext.Provider value={{ apiKey, isAuthenticated, login, logout }}>
      {children}
    </AuthContext.Provider>
  );
}

export function useAuth() {
  const context = useContext(AuthContext);
  if (context === undefined) {
    throw new Error('useAuth must be used within an AuthProvider');
  }
  return context;
}
