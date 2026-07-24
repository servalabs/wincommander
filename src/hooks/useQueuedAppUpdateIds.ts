// src/hooks/useQueuedAppUpdateIds.ts
//
// React view of the module-level app-update in-flight queue (appUpdateQueue.ts).
// The dashboard uses this to drop app-update findings from Needs Attention /
// Fix All the instant an upgrade is queued anywhere in the app.

import { useEffect, useState } from "react";
import { getQueuedAppUpdateIds, subscribeAppUpdateQueue } from "../lib/appUpdateQueue";

export function useQueuedAppUpdateIds(): Set<string> {
  const [ids, setIds] = useState<Set<string>>(() => getQueuedAppUpdateIds());
  useEffect(() => subscribeAppUpdateQueue(() => setIds(getQueuedAppUpdateIds())), []);
  return ids;
}

export default useQueuedAppUpdateIds;
