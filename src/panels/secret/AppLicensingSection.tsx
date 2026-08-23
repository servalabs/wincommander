// src/panels/secret/AppLicensingSection.tsx
//
// App Licensing card, split out of BrandingLicensingSection so it can sit
// beside the Hardening Presets card in the secret-grid (it filled the empty
// col-2/row-1 cell that the full-width "Change App Looks" card left open).

import { useCallback, useRef, useState } from "react";
import SectionCard from "../../components/shared/SectionCard";
import UniversalCallout from "../../components/shared/UniversalCallout";
import { Button } from "@/components/ui/bp";
import useEntitlements from "../../hooks/useEntitlements";
import useProInstall from "../../hooks/useProInstall";
import AppLicensePanel from "../identity/components/AppLicensePanel";

export default function AppLicensingSection() {
    const { hasPaid } = useEntitlements();
    const { isInstalled: proInstalled } = useProInstall({
        status: hasPaid,
        manifest: false,
        defender: false,
    });
    const [isLicenseOpen, setIsLicenseOpen] = useState(true);

    const licenseAutoSeededRef = useRef(false);
    const handleStatusLoaded = useCallback((status: any) => {
        if (!status?.licensed || !status?.valid) {
            setIsLicenseOpen(true);
            licenseAutoSeededRef.current = true;
            return;
        }
        if (status.expires_at) {
            const remaining = status.expires_at - (Date.now() / 1000);
            if (remaining < 1209600) {
                setIsLicenseOpen(true);
                licenseAutoSeededRef.current = true;
                return;
            }
        }
        if (!licenseAutoSeededRef.current) {
            licenseAutoSeededRef.current = true;
            setIsLicenseOpen(true);
        }
    }, []);

    return (
        <SectionCard
            title="App Licensing"
            icon="endorsed"
            collapsible
            isOpen={isLicenseOpen}
            onToggle={() => setIsLicenseOpen(!isLicenseOpen)}
        >
            <UniversalCallout
                message="Licensing is verified by our server."
                intent="primary"
                className="mb-4"
            />
            <AppLicensePanel onStatusLoaded={handleStatusLoaded} />
            {hasPaid && !proInstalled && (
                <div className="pro-reinstall-row mt-4">
                    <UniversalCallout
                        message="Your licence is active but the Pro features aren't installed on this PC yet."
                        intent="primary"
                        className="mb-3"
                    />
                    <Button
                        icon="cloud-download"
                        intent="primary"
                        text="Install Pro features"
                        onClick={() => window.dispatchEvent(new CustomEvent("pro-install-open"))}
                    />
                </div>
            )}
        </SectionCard>
    );
}
