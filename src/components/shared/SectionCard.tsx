import { Card, Icon, IconName, Collapse } from "@/components/ui/bp";
import { CSSProperties, ReactNode, useCallback, useState, useRef } from "react";
import './SectionCard.css';

interface SectionCardProps {
    title: string;
    children: ReactNode;
    className?: string;
    style?: CSSProperties;
    icon?: ReactNode | IconName;
    headerRight?: ReactNode;
    /** When true — all toggles in this section are armed. Adds accent underline to header. */
    armed?: boolean;
    /** Enable collapsible behavior */
    collapsible?: boolean;
    /** External control for expansion state (manual or accordion mode) */
    isOpen?: boolean;
    /** Callback for toggle interaction */
    onToggle?: () => void;
}

export default function SectionCard({
    title,
    children,
    className,
    style,
    icon,
    headerRight,
    armed,
    collapsible,
    isOpen = true,
    onToggle
}: SectionCardProps) {
    const [pulsing, setPulsing] = useState(false);
    const pulseTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

    const renderIcon = () => {
        if (!icon) return null;
        if (typeof icon === 'string') return <Icon icon={icon as IconName} />;
        return icon;
    };

    /** Called by child UniversalToggles via the onPulse prop */
    const handlePulse = useCallback(() => {
        if (pulseTimerRef.current) clearTimeout(pulseTimerRef.current);
        setPulsing(true);
        pulseTimerRef.current = setTimeout(() => setPulsing(false), 320);
    }, []);

    const isInternalOpen = collapsible ? isOpen : true;

    return (
        <Card
            className={`section-card ${pulsing ? 'pulsing' : ''} ${armed ? 'armed' : ''} ${collapsible ? 'collapsible' : ''} ${!isInternalOpen ? 'is-closed' : 'is-open'} ${className || ''}`}
            style={style}
            data-section-card="true"
        >
            <div
                className={`section-header-row ${collapsible ? 'cursor-pointer hover:bg-white/5' : ''}`}
                onClick={() => collapsible && onToggle?.()}
                role={collapsible ? "button" : undefined}
                tabIndex={collapsible ? 0 : undefined}
                aria-expanded={collapsible ? isInternalOpen : undefined}
                onKeyDown={collapsible ? (e) => {
                    if (e.key === "Enter" || e.key === " ") {
                        e.preventDefault();
                        onToggle?.();
                    }
                } : undefined}
            >
                <div className="section-header-left">
                    {icon && <span className="section-icon">{renderIcon()}</span>}
                    <div className="section-header-text">{title}</div>
                    {collapsible && (
                        <span className={`section-chevron${isInternalOpen ? " section-chevron--open" : ""}`}>
                            <Icon
                                icon="chevron-down"
                                size={14}
                                style={{ opacity: 0.5, marginLeft: 4 }}
                            />
                        </span>
                    )}
                </div>
                {headerRight && (
                    <div className="section-header-right" onClick={(e) => e.stopPropagation()}>
                        {headerRight}
                    </div>
                )}
            </div>
            
            <Collapse isOpen={isInternalOpen} className="section-collapse">
                <div className="section-content">
                    {typeof children === 'function'
                        ? (children as (pulse: () => void) => ReactNode)(handlePulse)
                        : children}
                </div>
            </Collapse>
        </Card>
    );
}

