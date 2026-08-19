import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-shell";
import { Button } from "@/components/ui/button";
import { Icon } from "@/components/ui/icon";
import { getSettingsOnce } from "@/hooks/useSettings";

function consoleOrigin(rawUrl: string): string | null {
  try {
    const url = new URL(rawUrl);
    return url.protocol === "http:" || url.protocol === "https:" ? url.origin : null;
  } catch {
    return null;
  }
}

export default function FleetConsoleLink() {
  const [serverUrl, setServerUrl] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    void getSettingsOnce().then(settings => {
      if (active) setServerUrl(consoleOrigin(settings.app?.fleet?.serverUrl ?? ""));
    }).catch(() => {
      if (active) setServerUrl(null);
    });
    return () => { active = false; };
  }, []);

  return (
    <section className="fleet-console-card" aria-label="Fleet management console">
      <div>
        <h3>Organization administration</h3>
        <p>
          Clipboard Guard and Ink Receipt policies are managed in the Fleet server console,
          not on enrolled devices.
        </p>
      </div>
      <Button
        onClick={() => { if (serverUrl) void open(serverUrl); }}
        disabled={!serverUrl}
      >
        <Icon icon="share" size={14} /> Open Fleet console
      </Button>
      {!serverUrl && <small>Enter and save a Fleet server URL to enable this link.</small>}
    </section>
  );
}
