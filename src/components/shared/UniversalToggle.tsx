// Compatibility shim — renders ToggleTile with the same props.
// Keeps all existing callsites working without any changes.
import ToggleTile from "./ToggleTile";
import type { ToggleTileProps } from "./ToggleTile";

export type UniversalToggleProps = ToggleTileProps;

export default function UniversalToggle(props: ToggleTileProps) {
  return <ToggleTile {...props} />;
}
