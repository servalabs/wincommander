import { createContext, useCallback, useContext, useState, type ReactNode } from "react";
import { armBackend } from "../hooks/armBackend";

export type AuthMode = "real" | "decoy";

interface Ctx {
  mode:    AuthMode;
  setMode: (m: AuthMode) => void;
}

const AuthModeContext = createContext<Ctx>({ mode: "real", setMode: () => {} });

export function AuthModeProvider({ children }: { children: ReactNode }) {
  const [mode, setModeState] = useState<AuthMode>("real");
  // Arm/disarm the backend DECOY_MODE backstop on EVERY decoy transition, not
  // just the calculator-gate path. Runtime triggers (typed distress phrase,
  // command palette) enter decoy through here too, and the backend must refuse
  // writes to (and later redact reads of) the real config the moment the decoy
  // view is shown — otherwise a coerced session could persist over the real
  // configuration via a direct invoke().
  const setMode = useCallback((m: AuthMode) => {
    // The view switch itself stays immediate (fail closed: never fall back to
    // the real view under duress). A failed backstop is logged and surfaced so
    // it is not silently assumed to be armed.
    setModeState(m);
    void armBackend(
      "set_decoy_mode",
      { on: m === "decoy" },
      m === "decoy"
        ? "Decoy protection could not be armed; real settings are NOT write-protected"
        : "Decoy protection could not be disarmed",
    );
  }, []);
  return (
    <AuthModeContext.Provider value={{ mode, setMode }}>
      {children}
    </AuthModeContext.Provider>
  );
}

export function useAuthMode(): Ctx {
  return useContext(AuthModeContext);
}
