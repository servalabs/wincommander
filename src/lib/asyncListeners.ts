type Unlisten = () => void | Promise<void>;

export function createAsyncListenerScope(onError: (error: unknown) => void) {
  const unlisteners = new Set<Unlisten>();
  let disposed = false;

  function unregister(unlisten: Unlisten) {
    try {
      // Tauri types this as void, but its runtime cleanup can reject asynchronously.
      void Promise.resolve(unlisten()).catch(onError);
    } catch (error) {
      onError(error);
    }
  }

  return {
    async listen<T>(register: (callback: (event: T) => void) => Promise<Unlisten>, callback: (event: T) => void) {
      if (disposed) return false;
      try {
        const unlisten = await register((event) => {
          if (!disposed) callback(event);
        });
        // React can tear down the effect before native registration replies.
        if (disposed) unregister(unlisten);
        else unlisteners.add(unlisten);
        return !disposed;
      } catch (error) {
        onError(error);
        return false;
      }
    },
    dispose() {
      if (disposed) return;
      disposed = true;
      for (const unlisten of unlisteners) unregister(unlisten);
      unlisteners.clear();
    },
  };
}
