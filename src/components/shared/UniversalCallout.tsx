import { Icon, IconName } from "@/components/ui/bp";
import './UniversalCallout.css';

interface UniversalCalloutProps {
    message: string;
    intent?: "primary" | "success" | "warning" | "danger";
    icon?: IconName;
    className?: string;
}

export default function UniversalCallout({ message, intent = "warning", icon, className = "" }: UniversalCalloutProps) {
    const defaultIcons: Record<string, IconName> = {
        primary: "info-sign",
        success: "tick-circle",
        warning: "warning-sign",
        danger: "error"
    };

    const displayIcon = icon || defaultIcons[intent];

    return (
        <div className={`universal-callout universal-callout-${intent} ${className}`}>
            <Icon icon={displayIcon} size={16} className="callout-icon" />
            <span className="callout-message">{message}</span>
        </div>
    );
}
