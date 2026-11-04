import { useFleetServiceContact } from "@/hooks/useFleetServiceContact";

function reachabilityMessage(state: string): string {
  switch (state) {
    case "network_configuration_differs": return "Service reachability is paused because proxy or certificate settings differ from the approved Fleet connection. WinCommander can still check in; an administrator can align the service's network settings.";
    case "transport_security_invalid": return "Service reachability is unconfirmed because its Fleet transport security settings are invalid. WinCommander can still check in.";
    case "request_rejected": return "Fleet rejected the service reachability request. The service will retry; an administrator can check device approval and the device clock.";
    case "server_unsupported": return "The Fleet server does not expose service reachability. The service will retry; an administrator can update Fleet.";
    case "network_unavailable": return "The service could not reach Fleet and will retry. WinCommander can still check in when connected.";
    case "proof_unverified": return "The service could not verify Fleet's reachability response and will retry.";
    case "credential_unavailable": return "The service has no usable Fleet credential yet. It can be refreshed after WinCommander establishes a verified connection.";
    case "service_store_unavailable": return "The service cannot read its protected Fleet settings and will retry.";
    case "checking": return "The service is checking Fleet reachability.";
    default: return "Service reachability is unconfirmed.";
  }
}

/** These observations are historical, not a silence timer or action. */
export function FleetServiceContact({ deviceId }: { deviceId: string }) {
  const contact = useFleetServiceContact(deviceId);
  const observation = contact?.ms ? new Date(contact.ms) : null;
  const serviceTime = contact?.serviceMs ? new Date(contact.serviceMs) : null;
  return <>
    {observation ? <p className="fleet-connect-enrolling">Last service-verified contact for this machine&apos;s Fleet service trust: <time dateTime={observation.toISOString()}>{observation.toLocaleString()}</time>. Historical device time for the service&apos;s enrolled trust; it can differ from the current Fleet connection. Automatic silence protection remains unavailable.</p>
      : <p className="fleet-connect-enrolling">Service-verified contact is unconfirmed.</p>}
    {contact?.serviceState === "observed" && serviceTime
      ? <p className="fleet-connect-enrolling">Last service reachability receipt for this machine&apos;s Fleet service trust: <time dateTime={serviceTime.toISOString()}>{serviceTime.toLocaleString()}</time>. This history can differ from the current Fleet connection; it does not prove operator presence or automatic protection.</p>
      : <p className="fleet-connect-enrolling">{reachabilityMessage(contact?.serviceState ?? "unconfirmed")}</p>}
  </>;
}
