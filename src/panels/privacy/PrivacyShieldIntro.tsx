import { Dialog, Button } from "@/components/ui/bp";
import PrivacyShieldAnimation from "./PrivacyShieldAnimation";

interface PrivacyShieldIntroProps {
    isOpen: boolean;
    onClose: () => void;
}

export default function PrivacyShieldIntro({ isOpen, onClose }: PrivacyShieldIntroProps) {

    return (
        <Dialog
            isOpen={isOpen}
            onClose={onClose}
            style={{ 
                width: 500, 
                background: 'var(--color-bg-secondary)',
                border: '1px solid var(--color-border)',
                borderRadius: '8px'
            }}
            title="What is Privacy Gaze Shield?"
        >
            <div className="p-6 flex flex-col items-center gap-6">
                
                {/* Animation Canvas — same box treatment (height/radius/bg/border)
                    as the tour's .spotlight-component-box (SpotlightTour.css) so
                    this dialog and the onboarding tour step present the identical
                    animation identically framed. */}
                <div className="relative w-full overflow-hidden rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-tertiary)] flex justify-center items-center" style={{ height: 260 }}>
                    <PrivacyShieldAnimation />
                </div>

                <div className="text-center max-w-[400px]">
                    <h3 className="text-sm font-semibold text-[var(--color-accent)] mb-2">
                        Instantly blurs when threat detected
                    </h3>
                    <p className="text-xs text-[var(--color-text-muted)] leading-relaxed">
                        Privacy Gaze Shield uses local AI to detect faces and camera lenses without ever sending data to the cloud. When a threat is detected, it instantly blurs your screen to protect your sensitive information.
                    </p>
                </div>

                <Button 
                    text="Got it" 
                    className="w-full mt-4 intro-btn-solid" 
                    onClick={onClose} 
                />
            </div>
        </Dialog>
    );
}
