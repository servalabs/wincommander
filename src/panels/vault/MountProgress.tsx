import { useEffect, useState } from "react";
import { Icon } from "@/components/ui/bp";
import { mountProgressMessage, type MountStage } from "./mountOperationProgress";

export default function MountProgress({ stage, customPim }: { stage: MountStage; customPim: boolean }) {
  const [started] = useState(() => Date.now());
  const [seconds, setSeconds] = useState(0);
  useEffect(() => {
    const timer = window.setInterval(() => setSeconds(Math.floor((Date.now() - started) / 1000)), 1000);
    return () => window.clearInterval(timer);
  }, [started]);
  return <div className="mount-progress" role="status">
    <Icon icon="time" size={16} />
    <span>{mountProgressMessage(stage, seconds, customPim)}</span>
  </div>;
}
