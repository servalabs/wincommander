import { useFleetServiceContact } from "@/hooks/useFleetServiceContact";

/** This is a historical service observation, not a silence timer or action. */
export function FleetServiceContact({ deviceId }: { deviceId: string }) {
  const observation = useFleetServiceContact(deviceId);
  if (observation === null) {
    return <p className="fleet-connect-enrolling">Service-verified contact is unconfirmed.</p>;
  }
  const time = new Date(observation);
  return <p className="fleet-connect-enrolling">Last service-verified contact for this machine&apos;s Fleet service trust: <time dateTime={time.toISOString()}>{time.toLocaleString()}</time>. Historical device time for the service&apos;s enrolled trust; it can differ from the current Fleet connection. Automatic silence protection remains unavailable.</p>;
}
