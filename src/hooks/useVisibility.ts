import { useMemo } from "react";
import { useAppState } from "../context/AppContext";
import { getCapabilitiesForSettings, getDensityForSettings, getDependencyIds } from "../lib/personaMigration";
import { isVisible, type Visibility, type VisibilityCtx } from "../lib/visibility";
export interface UseVisibilityResult extends VisibilityCtx {
  isVisible: (visibility?: Visibility) => boolean;
}

export default function useVisibility(): UseVisibilityResult {
  const { appSettings, dependencyStatus } = useAppState();

  const density = getDensityForSettings(appSettings);
  const profiles = useMemo(
    () => new Set(getCapabilitiesForSettings(appSettings)),
    [appSettings],
  );
  const dependencies = useMemo(
    () => getDependencyIds(dependencyStatus),
    [dependencyStatus],
  );

  const ctx = useMemo<VisibilityCtx>(
    () => ({
      density,
      profiles,
      dependencies,
    }),
    [density, dependencies, profiles],
  );

  const canSee = useMemo(() => {
    return (visibility?: Visibility) => isVisible(visibility, ctx);
  }, [ctx]);

  return {
    ...ctx,
    isVisible: canSee,
  };
}
