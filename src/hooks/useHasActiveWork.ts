// Combines every signal that means "the app is doing something the user
// would be unhappy to have paused":
//   1. trackBackendWork in-flight count (covers every run_backend_script
//      invocation — toggles, hardening, app installs, bleachbit, etc).
//   2. TaskStatusContext.tasks running (covers runOperation flows like
//      self-destruct, volume create, shred, that show in TaskStatusBar).
//
// Used by the idle-pause timer in App.tsx to defer pause while work is in
// flight, and to surface a "task in progress" hint in the status bar.

import { useTaskStatus } from "../context/TaskStatusContext";
import { useHasActiveBackendWork } from "../lib/activityStore";

export function useHasActiveWork(): boolean {
  const backendBusy = useHasActiveBackendWork();
  const { tasks } = useTaskStatus();
  const operationsBusy = tasks.some((t) => t.status === "running");
  return backendBusy || operationsBusy;
}
