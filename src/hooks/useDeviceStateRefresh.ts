import { useEffect } from "react";
import { onDeviceStateRefresh } from "../lib/deviceStateRefresh";

/**
 * Re-run a status read as soon as the device was changed outside this view
 * (for example by a Fleet command), in addition to the view's own polling.
 */
export default function useDeviceStateRefresh(refresh: () => void | Promise<void>) {
  useEffect(() => onDeviceStateRefresh(() => { void refresh(); }), [refresh]);
}
