// Narrow Blueprint-API bridge for the few eager legacy controls. New UI
// should import ./button directly; keeping this bridge focused avoids making
// the full bp compatibility barrel part of the startup graph.
import * as React from "react";
import { cn } from "../../lib/utils";
import { Button as UiButton } from "./button";
import { Icon, type IconName } from "./icon";
import { Spinner } from "./spinner";

type Intent = "none" | "primary" | "success" | "warning" | "danger";
type ButtonSize = "default" | "sm" | "lg" | "icon";

export interface CompatButtonProps {
  icon?: IconName | React.ReactElement;
  rightIcon?: IconName | React.ReactElement;
  text?: React.ReactNode;
  intent?: Intent;
  minimal?: boolean;
  outlined?: boolean;
  large?: boolean;
  small?: boolean;
  fill?: boolean;
  loading?: boolean;
  disabled?: boolean;
  active?: boolean;
  className?: string;
  onClick?: React.MouseEventHandler;
  onBlur?: React.FocusEventHandler;
  type?: "button" | "submit" | "reset";
  title?: string;
  "aria-label"?: string;
  "aria-pressed"?: boolean;
  tabIndex?: number;
  autoFocus?: boolean;
  style?: React.CSSProperties;
  size?: ButtonSize | "small" | "large";
  variant?: "default" | "primary" | "ghost" | "danger" | "outline";
  id?: string;
  name?: string;
  children?: React.ReactNode;
}

function variantFor(intent: Intent | undefined, minimal?: boolean, outlined?: boolean) {
  if (intent === "danger") return "danger" as const;
  if (intent === "primary") return "primary" as const;
  if (outlined) return "outline" as const;
  return minimal ? "ghost" as const : "default" as const;
}

function intentClass(intent?: Intent) {
  return intent === "success" ? "!text-[var(--ok)]" : intent === "warning" ? "!text-[var(--warn)]" : "";
}

function renderIcon(icon: CompatButtonProps["icon"], size = 16) {
  return typeof icon === "string" ? <Icon icon={icon} size={size} /> : icon ?? null;
}

export function CompatButton({
  icon, rightIcon, text, intent, minimal, outlined, large, small, fill, loading,
  disabled, active, className, onClick, onBlur, type = "button", title,
  "aria-label": ariaLabel, "aria-pressed": ariaPressed, tabIndex, autoFocus,
  style, size, variant, id, name, children,
}: CompatButtonProps) {
  const hasContent = text != null || children != null;
  const requestedSize = size === "large" ? "lg" : size === "small" ? "sm" : size;
  const buttonSize = requestedSize ?? (large ? "lg" : small ? "sm" : !hasContent && icon != null ? "icon" : "default");
  return (
    <UiButton
      type={type} variant={variant ?? variantFor(intent, minimal, outlined)} size={buttonSize}
      disabled={disabled || loading} onClick={onClick} onBlur={onBlur} title={title}
      aria-label={ariaLabel} aria-pressed={ariaPressed} tabIndex={tabIndex} autoFocus={autoFocus}
      style={style} id={id} name={name}
      className={cn(fill && "w-full", active && "bg-[var(--surface-3)] border-[var(--border-strong)]", intentClass(intent), className)}
    >
      {loading ? <Spinner size={16} /> : renderIcon(icon)}
      {text != null ? text : children}
      {!loading && renderIcon(rightIcon)}
    </UiButton>
  );
}
