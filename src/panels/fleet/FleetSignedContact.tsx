/** A saved signed-contact observation is historical, even when the link is online. */
export function FleetSignedContact({ timestamp }: { timestamp: unknown }) {
  if (typeof timestamp === "string") {
    const date = new Date(timestamp);
    if (Number.isFinite(date.getTime())) {
      return (
        <p className="fleet-connect-enrolling">
          <span>
            Last verified Fleet contact: <time dateTime={timestamp}>{date.toLocaleString()}</time>. Historical time from this device; it does not indicate the current connection.
          </span>
        </p>
      );
    }
  }

  return (
    <p className="fleet-connect-enrolling">
      Verified Fleet contact is unavailable in this status.
    </p>
  );
}
