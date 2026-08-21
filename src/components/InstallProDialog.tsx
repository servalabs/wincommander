// src/components/InstallProDialog.tsx
//
// ═══════════════════════════════════════════════════════════════════════
// INSTALL PRO DIALOG — One-click Pro sidecar install after activation
// ═══════════════════════════════════════════════════════════════════════
//
// Three states: consent / installing / done (or error). Triggered by
// the global `pro-install-open` window event so any surface can request
// it: the LicenseGate fires it after a successful licence activation if
// Pro isn't already on disk; the LicenseQuickPanel fires it from a
// "Install Pro" button when a paid licence is active without a sidecar.
//
// The Rust install_pro_binary command does the actual download + verify
// + atomic-rename in a single round-trip, so we don't surface granular
// progress -- just a spinner with the short "Installing..." copy.

import { CompatDialog as Dialog, CompatDialogBody as DialogBody, CompatDialogFooter as DialogFooter } from "@/components/ui/compat-dialog";
import { useEffect, useState } from "react";
import useProInstall from "../hooks/useProInstall";
import { renderProInstallStep } from "./shared/ProInstallStepBody";

interface InstallProDialogProps {
    isOpen: boolean;
    onClose: () => void;
    /** Called instead of onClose when the user clicks "Not now" from the
     *  startup-nag path. Lets App.tsx persist the dismissed flag so the nag
     *  doesn't repeat on subsequent startups. On-demand invocations (paid-cmd
     *  failures) leave this undefined, so "Not now" just closes normally. */
    onNotNow?: () => void;
}

export default function InstallProDialog({ isOpen, onClose, onNotNow }: InstallProDialogProps) {
    const pro = useProInstall();
    const [consent, setConsent] = useState(false);

    // Reset the consent checkbox when the dialog closes so reopening
    // shows a fresh prompt. Don't clear installState on close -- if the
    // user closes mid-install (shouldn't be possible, but defensive)
    // they should still see the result on reopen.
    useEffect(() => {
        if (!isOpen) setConsent(false);
    }, [isOpen]);

    // Step content (consent / installing / installed / error) is shared with
    // UpdateFlowDialog's embedded Pro step — see ProInstallStepBody.tsx.
    const { body, footer, isBusy } = renderProInstallStep({
        pro,
        consent,
        onConsentChange: setConsent,
        onNotNow,
        onClose,
    });

    return (
        <Dialog
            isOpen={isOpen}
            onClose={isBusy ? undefined : onClose}
            title="Install WinCommander Pro"
            icon="cloud-download"
            canEscapeKeyClose={!isBusy}
            canOutsideClickClose={!isBusy}
            style={{ width: 520 }}
        >
            <DialogBody>{body}</DialogBody>
            <DialogFooter>{footer}</DialogFooter>
        </Dialog>
    );
}
