// bp.tsx — BlueprintJS-API-compatible compatibility layer.
//
// WinCommander V2 is migrating off @blueprintjs/core. ~100 files import
// Blueprint components (Button, Dialog, Tooltip, Tag, …) with Blueprint's
// prop API. Rather than rewrite every call site, this shim re-exposes the
// Blueprint names + signatures, backed by the existing V2 shadcn/Radix kit
// in this folder. Call sites only change their import source to
// "@/components/ui/bp" (or a relative path) and keep their current props.
//
// Backing primitives: ./button, ./switch, ./badge, ./tooltip, ./popover,
// ./icon, ./spinner, and Radix primitives directly (dialog / alert-dialog /
// checkbox) where Blueprint's controlled pattern needs it.

import * as React from "react";
import * as RDialog from "@radix-ui/react-dialog";
import * as RAlertDialog from "@radix-ui/react-alert-dialog";
import * as CheckboxPrimitive from "@radix-ui/react-checkbox";
import { ChevronDown, X } from "lucide-react";

import { cn } from "../../lib/utils";
import { DURATION } from "../shared/motion";
import { Button as UiButton } from "./button";
import { Switch as UiSwitch } from "./switch";
import { Badge } from "./badge";
import { Spinner as UiSpinner } from "./spinner";
import { Icon, type IconName } from "./icon";
import {
  Tooltip as UiTooltip,
  TooltipTrigger,
  TooltipContent,
} from "./tooltip";
import {
  Popover as UiPopover,
  PopoverTrigger,
  PopoverContent,
} from "./popover";
import { Slider as UiSlider } from "./slider";

// ── Re-exports of shared primitives ───────────────────────────────────────
export { Icon };
export type { IconName };

// ───────────────────────────────────────────────────────────────────────────
// Shared types & helpers
// ───────────────────────────────────────────────────────────────────────────

// Intent: declaration-merged type + const value (Blueprint exposes both).
export type Intent = "none" | "primary" | "success" | "warning" | "danger";
export const Intent = {
  NONE: "none",
  PRIMARY: "primary",
  SUCCESS: "success",
  WARNING: "warning",
  DANGER: "danger",
} as const;

/** Map an intent to its CSS color var. */
function intentColor(intent?: Intent): string {
  switch (intent) {
    case "primary":
      return "var(--accent)";
    case "success":
      return "var(--ok)";
    case "warning":
      return "var(--warn)";
    case "danger":
      return "var(--danger)";
    default:
      return "var(--text-mute)";
  }
}

/** Map a Blueprint Position/placement string to a Radix `side`. */
function mapPosition(p?: string): "top" | "bottom" | "left" | "right" {
  if (!p) return "bottom";
  if (p.startsWith("top")) return "top";
  if (p.startsWith("left")) return "left";
  if (p.startsWith("right")) return "right";
  return "bottom";
}

function renderTriggerChild(children: React.ReactNode, fill?: boolean): React.ReactElement {
  // KT: Radix `asChild` clones its target and injects props like `type` and
  // `aria-describedby`. React.Fragment cannot receive those props, so fragment,
  // text, and multi-child Blueprint targets need a real wrapper element.
  if (fill) return <span className="w-full">{children}</span>;

  const onlyChild = React.Children.count(children) === 1
    ? React.Children.toArray(children)[0]
    : null;

  if (React.isValidElement(onlyChild) && onlyChild.type !== React.Fragment) {
    return onlyChild;
  }

  return <span className="inline-flex">{children}</span>;
}

// Classes — Blueprint's class-name registry. Stray `Classes.FOO` reads return
// "" so leftover className usages compile and render harmlessly. A Proxy keeps
// any unknown key safe.
export const Classes: Record<string, string> = new Proxy(
  {
    DARK: "",
    DIALOG: "",
    DIALOG_BODY: "",
    DIALOG_FOOTER: "",
    DIALOG_FOOTER_ACTIONS: "",
    POPOVER_DISMISS: "",
    MINIMAL: "",
  } as Record<string, string>,
  {
    get(target, key: string) {
      return key in target ? target[key] : "";
    },
  }
);

// Position — Blueprint's position enum, collapsed onto Radix sides.
export const Position = {
  TOP: "top",
  BOTTOM: "bottom",
  LEFT: "left",
  RIGHT: "right",
  BOTTOM_RIGHT: "bottom",
  BOTTOM_LEFT: "bottom",
  TOP_RIGHT: "top",
  TOP_LEFT: "top",
  LEFT_TOP: "left",
  RIGHT_TOP: "right",
} as const;

// ───────────────────────────────────────────────────────────────────────────
// Button / AnchorButton / ButtonGroup
// ───────────────────────────────────────────────────────────────────────────

type ButtonVariant = "default" | "primary" | "ghost" | "danger" | "outline";
type ButtonSize = "default" | "sm" | "lg" | "icon";

interface ButtonProps {
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
  type?: "button" | "submit" | "reset";
  title?: string;
  "aria-label"?: string;
  tabIndex?: number;
  autoFocus?: boolean;
  style?: React.CSSProperties;
  size?: string;
  variant?: string;
  onBlur?: React.FocusEventHandler;
  id?: string;
  name?: string;
  "data-tip"?: string;
  /** Guide-tour anchor, e.g. '[data-tour="privacy-shield-how-it-works"]'. */
  "data-tour"?: string;
  /** Guide-tour real-time state opt-in ("scanning" | "done") — read by
   *  useTour's anchor resolver to swap in alternate copy / auto-unlock Next.
   *  See useTour.ts's `find()` for the contract. */
  "data-tour-state"?: string;
  children?: React.ReactNode;
}

function pickVariant(intent: Intent | undefined, minimal?: boolean, outlined?: boolean): ButtonVariant {
  if (intent === "danger") return "danger";
  if (intent === "primary") return "primary";
  // success/warning aren't first-class variants — render default and tint via
  // className (handled by the caller via intentClass below).
  if (outlined) return "outline";
  if (minimal) return "ghost";
  return "default";
}

/** Extra color class for success/warning intents that lack a real variant. */
function intentClass(intent?: Intent): string {
  if (intent === "success") return "!text-[var(--ok)]";
  if (intent === "warning") return "!text-[var(--warn)]";
  return "";
}

function renderIcon(icon: IconName | React.ReactElement | undefined, size = 16) {
  if (icon == null) return null;
  if (typeof icon === "string") return <Icon icon={icon} size={size} />;
  return icon;
}

export function Button({
  icon,
  rightIcon,
  text,
  intent,
  minimal,
  outlined,
  large,
  small,
  fill,
  loading,
  disabled,
  active,
  className,
  onClick,
  type = "button",
  title,
  "aria-label": ariaLabel,
  tabIndex,
  autoFocus,
  style,
  children,
  "data-tip": dataTip,
  "data-tour": dataTour,
  "data-tour-state": dataTourState,
}: ButtonProps) {
  const hasContent = text != null || children != null;
  const variant = pickVariant(intent, minimal, outlined);
  const btnSize: ButtonSize = large ? "lg" : small ? "sm" : !hasContent && icon != null ? "icon" : "default";

  return (
    <UiButton
      type={type}
      variant={variant}
      size={btnSize}
      disabled={disabled || loading}
      onClick={onClick}
      title={title}
      aria-label={ariaLabel}
      tabIndex={tabIndex}
      autoFocus={autoFocus}
      style={style}
      data-tip={dataTip}
      data-tour={dataTour}
      data-tour-state={dataTourState}
      className={cn(
        fill && "w-full",
        active && "bg-[var(--surface-3)] border-[var(--border-strong)]",
        intentClass(intent),
        className
      )}
    >
      {loading ? <UiSpinner size={16} /> : renderIcon(icon)}
      {text != null ? text : children}
      {!loading && renderIcon(rightIcon)}
    </UiButton>
  );
}

interface AnchorButtonProps extends Omit<ButtonProps, "type"> {
  href?: string;
  target?: string;
  rel?: string;
}

export function AnchorButton({
  icon,
  rightIcon,
  text,
  intent,
  minimal,
  outlined,
  large,
  small,
  fill,
  loading,
  disabled,
  active,
  className,
  onClick,
  title,
  "aria-label": ariaLabel,
  tabIndex,
  href,
  target,
  rel,
  children,
}: AnchorButtonProps) {
  const hasContent = text != null || children != null;
  const variant = pickVariant(intent, minimal, outlined);
  const size: ButtonSize = large ? "lg" : small ? "sm" : !hasContent && icon != null ? "icon" : "default";

  return (
    <UiButton
      asChild
      variant={variant}
      size={size}
      className={cn(
        fill && "w-full",
        active && "bg-[var(--surface-3)] border-[var(--border-strong)]",
        (disabled || loading) && "opacity-50 pointer-events-none",
        intentClass(intent),
        className
      )}
    >
      <a
        href={href}
        target={target}
        rel={rel}
        onClick={onClick}
        title={title}
        aria-label={ariaLabel}
        tabIndex={tabIndex}
        aria-disabled={disabled || loading || undefined}
      >
        {loading ? <UiSpinner size={16} /> : renderIcon(icon)}
        {text != null ? text : children}
        {!loading && renderIcon(rightIcon)}
      </a>
    </UiButton>
  );
}

export function ButtonGroup({
  children,
  fill,
  vertical,
  className,
}: {
  children?: React.ReactNode;
  minimal?: boolean;
  fill?: boolean;
  vertical?: boolean;
  large?: boolean;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "inline-flex gap-1",
        vertical && "flex-col",
        fill && "w-full [&>*]:flex-1",
        className
      )}
    >
      {children}
    </div>
  );
}

// ───────────────────────────────────────────────────────────────────────────
// Switch
// ───────────────────────────────────────────────────────────────────────────

/** Synthesize a minimal React change event from a boolean checked state. */
function syntheticChangeEvent(
  checked: boolean
): React.ChangeEvent<HTMLInputElement> {
  return {
    target: { checked },
    currentTarget: { checked },
  } as unknown as React.ChangeEvent<HTMLInputElement>;
}

export function Switch({
  checked,
  defaultChecked,
  onChange,
  label,
  disabled,
  className,
  style,
}: {
  checked?: boolean;
  defaultChecked?: boolean;
  onChange?: (e: React.ChangeEvent<HTMLInputElement>) => void;
  label?: React.ReactNode;
  disabled?: boolean;
  inline?: boolean;
  large?: boolean;
  className?: string;
  alignIndicator?: string;
  style?: React.CSSProperties;
  innerLabel?: React.ReactNode;
  innerLabelChecked?: React.ReactNode;
}) {
  return (
    <label style={style} className={cn("inline-flex items-center gap-2 cursor-pointer", className)}>
      <UiSwitch
        checked={checked}
        defaultChecked={defaultChecked}
        disabled={disabled}
        onCheckedChange={(c) => onChange?.(syntheticChangeEvent(c))}
      />
      {label != null && (
        <span className="text-[13px] text-[var(--text-dim)]">{label}</span>
      )}
    </label>
  );
}

// ───────────────────────────────────────────────────────────────────────────
// Checkbox
// ───────────────────────────────────────────────────────────────────────────

// CheckDrawSvg — SVG check that "draws in" via stroke-dashoffset animation.
// Remounting this element (via key change) replays the animation on re-check.
// wc-no-motion collapses animation-duration to 0.01ms (v2-theme.css rule),
// so the mark appears instantly under reduced motion — no JS branch needed.
//
// The keyframe lives here (not in v2-theme.css) because bp.tsx is the only
// consumer of wc-check-draw; co-locating avoids an orphan CSS rule.
const CHECK_KEYFRAME = `@keyframes wc-check-draw{to{stroke-dashoffset:0}}`;
const CHECK_PATH = "M2 6 L5 9 L10 3"; // fits a 12×12 viewBox
const CHECK_PATH_LEN = 11; // approximate stroke length for dasharray

/** Injects the wc-check-draw keyframe once into the document head. */
function useCheckKeyframe() {
  React.useEffect(() => {
    const id = "wc-check-draw-style";
    if (document.getElementById(id)) return;
    const el = document.createElement("style");
    el.id = id;
    el.textContent = CHECK_KEYFRAME;
    document.head.appendChild(el);
    // No cleanup — the keyframe is app-global and tiny; leaving it avoids
    // re-injection cost when Checkbox mounts/unmounts frequently.
  }, []);
}

function CheckDrawSvg() {
  return (
    <svg
      viewBox="0 0 12 12"
      width={12}
      height={12}
      fill="none"
      aria-hidden="true"
      className="text-[var(--accent-contrast)]"
    >
      <path
        d={CHECK_PATH}
        stroke="currentColor"
        strokeWidth={1.75}
        strokeLinecap="round"
        strokeLinejoin="round"
        style={{
          strokeDasharray: CHECK_PATH_LEN,
          // Start fully hidden; the keyframe animates offset→0 to draw it in.
          strokeDashoffset: CHECK_PATH_LEN,
          animation: `wc-check-draw var(--dur-fast, ${DURATION.fast}ms) var(--ease) forwards`,
        }}
      />
    </svg>
  );
}

export function CheckboxControl({
  checked,
  defaultChecked,
  onChange,
  disabled,
  indeterminate,
  className,
  tabIndex,
  onClick,
  ariaLabel,
}: {
  checked?: boolean;
  defaultChecked?: boolean;
  onChange?: (e: React.ChangeEvent<HTMLInputElement>) => void;
  disabled?: boolean;
  indeterminate?: boolean;
  className?: string;
  tabIndex?: number;
  onClick?: React.MouseEventHandler<HTMLButtonElement>;
  ariaLabel?: string;
}) {
  useCheckKeyframe();

  const checkedState: CheckboxPrimitive.CheckedState | undefined =
    indeterminate ? "indeterminate" : checked;

  return (
    <CheckboxPrimitive.Root
      checked={checkedState}
      defaultChecked={defaultChecked}
      disabled={disabled}
      tabIndex={tabIndex}
      onClick={onClick}
      aria-label={ariaLabel}
      onCheckedChange={(c) =>
        onChange?.(syntheticChangeEvent(c === true))
      }
      // Box background/border transition is color-only — GPU-safe.
      // Uses CSS tokens so duration/ease stay in sync with the design system.
      className={cn(
        "size-4 shrink-0 rounded-[var(--r-sm)] border border-[var(--border-strong)] bg-[var(--surface-2)] data-[state=checked]:bg-[var(--accent)] data-[state=checked]:border-[var(--accent-line)] data-[state=indeterminate]:bg-[var(--accent)] data-[state=indeterminate]:border-[var(--accent-line)] grid place-items-center outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-line)] disabled:pointer-events-none transition-[background-color,border-color] [transition-duration:var(--dur-fast)] [transition-timing-function:var(--ease)]",
        className,
      )}
    >
      {/* forceMount keeps the indicator in the DOM so the exit transition
          on the box color is visible even as the indicator disappears. */}
      <CheckboxPrimitive.Indicator forceMount>
        {indeterminate ? (
          <span className="block h-0.5 w-2 rounded-full bg-[var(--accent-contrast)]" />
        ) : (
          // key on checkedState forces SVG remount → replays draw animation
          checkedState === true && <CheckDrawSvg key="checked" />
        )}
      </CheckboxPrimitive.Indicator>
    </CheckboxPrimitive.Root>
  );
}

export function Checkbox({
  checked,
  defaultChecked,
  onChange,
  label,
  disabled,
  indeterminate,
  className,
  style,
  children,
}: {
  checked?: boolean;
  defaultChecked?: boolean;
  onChange?: (e: React.ChangeEvent<HTMLInputElement>) => void;
  label?: React.ReactNode;
  disabled?: boolean;
  indeterminate?: boolean;
  inline?: boolean;
  className?: string;
  large?: boolean;
  style?: React.CSSProperties;
  children?: React.ReactNode;
}) {
  return (
    <label
      style={style}
      className={cn(
        "inline-flex items-center gap-2 cursor-pointer text-[13px] text-[var(--text-dim)]",
        disabled && "opacity-50 cursor-not-allowed",
        className
      )}
    >
      <CheckboxControl
        checked={checked}
        defaultChecked={defaultChecked}
        disabled={disabled}
        indeterminate={indeterminate}
        onChange={onChange}
      />
      {(label ?? children) != null && <span>{label ?? children}</span>}
    </label>
  );
}

// ───────────────────────────────────────────────────────────────────────────
// Dialog + DialogBody + DialogFooter
// ───────────────────────────────────────────────────────────────────────────

export function Dialog({
  isOpen,
  onClose,
  title,
  icon,
  canEscapeKeyClose = true,
  canOutsideClickClose = true,
  className,
  style,
  children,
  isCloseButtonShown = true,
  onOpened,
}: {
  isOpen?: boolean;
  onClose?: () => void;
  title?: React.ReactNode;
  icon?: IconName;
  canEscapeKeyClose?: boolean;
  canOutsideClickClose?: boolean;
  className?: string;
  style?: React.CSSProperties;
  children?: React.ReactNode;
  isCloseButtonShown?: boolean;
  portalClassName?: string;
  enforceFocus?: boolean;
  onOpened?: () => void;
  backdropProps?: unknown;
}) {
  return (
    <RDialog.Root
      open={isOpen}
      onOpenChange={(o) => {
        if (!o) onClose?.();
        else onOpened?.();
      }}
    >
      <RDialog.Portal>
        {/* Overlay: fades in/out symmetrically. Duration from CSS token.
            No backdrop-blur: a full-viewport blur recomposites on every
            frame of whatever's animating behind it, and on WebView2/software
            rendering (VM hosts) that GPU blur pass can leave stale smeared
            regions on screen. bg-black/70 keeps the dimming without it —
            same tradeoff already made for Sidebar/RightSidebar/dashboard. */}
        <RDialog.Overlay
          className="fixed inset-0 z-[var(--z-modal)] bg-black/70 data-[state=open]:animate-in data-[state=open]:fade-in-0 data-[state=closed]:animate-out data-[state=closed]:fade-out-0"
          style={{
            animationDuration: `var(--dur-normal, ${DURATION.normal}ms)`,
            animationTimingFunction: "var(--ease)",
          }}
        />
        {/* Content: opacity 0→1 + scale 0.94→1 on enter; mirrors on exit.
            zoom-in-95/zoom-out-95 ≈ scale(0.94) via tailwind-animate — no
            custom keyframe needed, keeps this file dependency-free. */}
        <RDialog.Content
          onEscapeKeyDown={(e) => {
            if (!canEscapeKeyClose) e.preventDefault();
          }}
          onPointerDownOutside={(e) => {
            if (!canOutsideClickClose) e.preventDefault();
          }}
          className={cn(
            // gpu-clip: isolation + will-change:transform — same Tauri WebView2
            // corner-bleed fix as Button/Chip, needed here because zoom-in-95/
            // zoom-out-95 scale-transforms the whole dialog on open/close.
            "gpu-clip fixed left-1/2 top-1/2 z-[var(--z-modal)] w-full max-w-lg -translate-x-1/2 -translate-y-1/2 rounded-[var(--r-lg)] border border-[var(--border-strong)] bg-[var(--surface)] text-[var(--text)] shadow-[var(--shadow)] outline-none data-[state=open]:animate-in data-[state=open]:fade-in-0 data-[state=open]:zoom-in-95 data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=closed]:zoom-out-95",
            className
          )}
          style={{
            ...style,
            animationDuration: `var(--dur-normal, ${DURATION.normal}ms)`,
            animationTimingFunction: "var(--ease)",
          }}
        >
          {title != null ? (
            <div className="flex items-center gap-2 px-5 py-3.5 border-b border-[var(--border)]">
              {icon != null && (
                <Icon icon={icon} size={16} className="text-[var(--text-dim)]" />
              )}
              <RDialog.Title className="font-[family-name:var(--font-display)] text-[15px] font-semibold text-[var(--text)] flex-1">
                {title}
              </RDialog.Title>
              {isCloseButtonShown && (
                <RDialog.Close
                  aria-label="Close"
                  className="grid size-7 place-items-center rounded-[var(--r-sm)] text-[var(--text-mute)] hover:bg-[var(--surface-2)] hover:text-[var(--text)] transition-colors"
                >
                  <X size={16} />
                </RDialog.Close>
              )}
            </div>
          ) : (
            // Radix requires a Title for a11y; provide a hidden one.
            <RDialog.Title className="sr-only">Dialog</RDialog.Title>
          )}
          {children}
        </RDialog.Content>
      </RDialog.Portal>
    </RDialog.Root>
  );
}

export function DialogBody({
  children,
  className,
}: {
  children?: React.ReactNode;
  className?: string;
  useOverflowScrollContainer?: boolean;
}) {
  return (
    <div className={cn("px-5 py-4 overflow-y-auto", className)}>{children}</div>
  );
}

export function DialogFooter({
  children,
  actions,
  className,
}: {
  children?: React.ReactNode;
  actions?: React.ReactNode;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "flex items-center justify-end gap-2 px-5 py-3 border-t border-[var(--border)]",
        className
      )}
    >
      {children}
      {actions}
    </div>
  );
}

// ───────────────────────────────────────────────────────────────────────────
// Alert
// ───────────────────────────────────────────────────────────────────────────

export function Alert({
  isOpen,
  onConfirm,
  onClose,
  onCancel,
  confirmButtonText = "Confirm",
  cancelButtonText,
  intent,
  icon,
  canEscapeKeyCancel = true,
  canOutsideClickCancel = true,
  children,
  className,
}: {
  isOpen?: boolean;
  onConfirm?: () => void;
  onClose?: () => void;
  onCancel?: () => void;
  confirmButtonText?: React.ReactNode;
  cancelButtonText?: React.ReactNode;
  intent?: Intent;
  icon?: IconName;
  canEscapeKeyCancel?: boolean;
  canOutsideClickCancel?: boolean;
  children?: React.ReactNode;
  className?: string;
  loading?: boolean;
}) {
  const close = () => {
    onCancel?.();
    onClose?.();
  };
  return (
    <RAlertDialog.Root
      open={isOpen}
      onOpenChange={(o) => {
        if (!o) close();
      }}
    >
      <RAlertDialog.Portal>
        {/* Overlay: symmetric fade. Duration from CSS token.
            No backdrop-blur — see Dialog.Overlay above for why. */}
        <RAlertDialog.Overlay
          className="fixed inset-0 z-[var(--z-modal)] bg-black/70 data-[state=open]:animate-in data-[state=open]:fade-in-0 data-[state=closed]:animate-out data-[state=closed]:fade-out-0"
          style={{
            animationDuration: `var(--dur-normal, ${DURATION.normal}ms)`,
            animationTimingFunction: "var(--ease)",
          }}
        />
        {/* Content: fade + scale enter AND exit (was enter-only before).
            zoom-out-95 adds the symmetric scale-down on close. */}
        <RAlertDialog.Content
          onEscapeKeyDown={(e) => {
            if (!canEscapeKeyCancel) e.preventDefault();
          }}
          className={cn(
            // gpu-clip: same Tauri WebView2 corner-bleed fix as Dialog.Content above.
            "gpu-clip fixed left-1/2 top-1/2 z-[var(--z-modal)] w-full max-w-md -translate-x-1/2 -translate-y-1/2 rounded-[var(--r-lg)] border border-[var(--border-strong)] bg-[var(--surface)] text-[var(--text)] shadow-[var(--shadow)] outline-none data-[state=open]:animate-in data-[state=open]:fade-in-0 data-[state=open]:zoom-in-95 data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=closed]:zoom-out-95",
            className
          )}
          style={{
            animationDuration: `var(--dur-normal, ${DURATION.normal}ms)`,
            animationTimingFunction: "var(--ease)",
          }}
        >
          <div className="flex gap-3 px-5 py-4">
            {icon != null && (
              <Icon
                icon={icon}
                size={20}
                color={intentColor(intent)}
                className="mt-0.5 shrink-0"
              />
            )}
            <div className="text-[13px] text-[var(--text-dim)] flex-1">
              <RAlertDialog.Title className="sr-only">Alert</RAlertDialog.Title>
              <RAlertDialog.Description asChild>
                <div>{children}</div>
              </RAlertDialog.Description>
            </div>
          </div>
          <div className="flex items-center justify-end gap-2 px-5 py-3 border-t border-[var(--border)]">
            {cancelButtonText != null && (
              <RAlertDialog.Cancel asChild>
                <Button
                  minimal
                  onClick={() => {
                    if (!canOutsideClickCancel) {
                      /* no-op: keep prop referenced */
                    }
                    onCancel?.();
                    onClose?.();
                  }}
                  text={cancelButtonText}
                />
              </RAlertDialog.Cancel>
            )}
            <RAlertDialog.Action asChild>
              <Button
                intent={intent}
                onClick={() => {
                  onConfirm?.();
                  onClose?.();
                }}
                text={confirmButtonText}
              />
            </RAlertDialog.Action>
          </div>
        </RAlertDialog.Content>
      </RAlertDialog.Portal>
    </RAlertDialog.Root>
  );
}

// ───────────────────────────────────────────────────────────────────────────
// Callout
// ───────────────────────────────────────────────────────────────────────────

const CALLOUT_DEFAULT_ICON: Record<Intent, IconName> = {
  none: "info-sign",
  primary: "info-sign",
  success: "tick-circle",
  warning: "warning-sign",
  danger: "error",
};

export function Callout({
  intent = "none",
  icon,
  title,
  children,
  className,
  style,
}: {
  intent?: Intent;
  icon?: IconName | null;
  title?: React.ReactNode;
  children?: React.ReactNode;
  className?: string;
  style?: React.CSSProperties;
}) {
  const color = intentColor(intent);
  const showIcon = icon !== null;
  const resolvedIcon: IconName = icon ?? CALLOUT_DEFAULT_ICON[intent];
  const tint =
    intent === "none"
      ? undefined
      : `color-mix(in srgb, ${color} 8%, transparent)`;

  return (
    <div
      className={cn(
        "rounded-[var(--r)] border-l-2 p-3 text-[13px]",
        intent === "none" && "border-[var(--border)] bg-[var(--surface-2)]",
        className
      )}
      style={
        intent === "none"
          ? style
          : { borderLeftColor: color, background: tint, ...style }
      }
    >
      <div className="flex gap-2">
        {showIcon && (
          <Icon
            icon={resolvedIcon}
            size={16}
            color={intent === "none" ? "var(--text-mute)" : color}
            className="mt-0.5 shrink-0"
          />
        )}
        <div className="flex-1 text-[var(--text-dim)]">
          {title != null && (
            <div className="font-semibold text-[var(--text)] mb-0.5">
              {title}
            </div>
          )}
          {children}
        </div>
      </div>
    </div>
  );
}

// ───────────────────────────────────────────────────────────────────────────
// Tag
// ───────────────────────────────────────────────────────────────────────────

type BadgeTone = "neutral" | "accent" | "success" | "warning" | "danger";

function intentTone(intent?: Intent): BadgeTone {
  switch (intent) {
    case "primary":
      return "accent";
    case "success":
      return "success";
    case "warning":
      return "warning";
    case "danger":
      return "danger";
    default:
      return "neutral";
  }
}

export function Tag({
  intent,
  round,
  icon,
  rightIcon,
  interactive,
  onClick,
  onRemove,
  className,
  style,
  title,
  children,
}: {
  intent?: Intent;
  minimal?: boolean;
  large?: boolean;
  round?: boolean;
  icon?: IconName | React.ReactElement;
  rightIcon?: IconName | React.ReactElement;
  interactive?: boolean;
  onClick?: React.MouseEventHandler;
  onRemove?: (e: React.MouseEvent<HTMLButtonElement>) => void;
  className?: string;
  style?: React.CSSProperties;
  title?: string;
  children?: React.ReactNode;
}) {
  const clickable = onClick != null;
  return (
    <Badge
      tone={intentTone(intent)}
      onClick={onClick}
      role={clickable ? "button" : undefined}
      tabIndex={clickable ? 0 : undefined}
      onKeyDown={
        clickable
          ? (e: React.KeyboardEvent<HTMLSpanElement>) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                e.currentTarget.click();
              }
            }
          : undefined
      }
      style={style}
      title={title}
      className={cn(
        round && "rounded-full",
        interactive && "cursor-pointer hover:brightness-110",
        className
      )}
    >
      {renderIcon(icon, 12)}
      {children}
      {renderIcon(rightIcon, 12)}
      {onRemove != null && (
        <button
          type="button"
          aria-label="Remove"
          onClick={onRemove}
          className="ml-0.5 -mr-0.5 grid place-items-center rounded-full opacity-70 hover:opacity-100"
        >
          <X size={11} />
        </button>
      )}
    </Badge>
  );
}

// ───────────────────────────────────────────────────────────────────────────
// Spinner
// ───────────────────────────────────────────────────────────────────────────

export function Spinner({
  size = 16,
  className,
  style,
}: {
  size?: number;
  intent?: string;
  className?: string;
  style?: React.CSSProperties;
}) {
  return <UiSpinner size={size} className={className} style={style} />;
}

// ───────────────────────────────────────────────────────────────────────────
// Tooltip
// ───────────────────────────────────────────────────────────────────────────

export function Tooltip({
  content,
  children,
  position,
  placement,
  disabled,
  className,
  fill,
}: {
  content?: React.ReactNode;
  children?: React.ReactNode;
  position?: string;
  placement?: string;
  disabled?: boolean;
  className?: string;
  hoverOpenDelay?: number;
  compact?: boolean;
  intent?: string;
  minimal?: boolean;
  fill?: boolean;
}) {
  if (disabled || content == null) return <>{children}</>;
  return (
    <UiTooltip>
      <TooltipTrigger asChild>
        {renderTriggerChild(children, fill)}
      </TooltipTrigger>
      <TooltipContent side={mapPosition(position ?? placement)} className={className}>
        {content}
      </TooltipContent>
    </UiTooltip>
  );
}

// ───────────────────────────────────────────────────────────────────────────
// Popover
// ───────────────────────────────────────────────────────────────────────────

// Props shared by both Popover paths.
interface PopoverSharedProps {
  content?: React.ReactNode;
  position?: string;
  placement?: string;
  isOpen?: boolean;
  onInteraction?: (nextOpen: boolean) => void;
  onClose?: () => void;
  popoverClassName?: string;
  onOpenAutoFocus?: React.ComponentProps<typeof PopoverContent>["onOpenAutoFocus"];
  onCloseAutoFocus?: React.ComponentProps<typeof PopoverContent>["onCloseAutoFocus"];
}

/** Preserve Blueprint's `-start` / `-end` suffix for Radix alignment. */
function mapAlignment(p?: string): "start" | "center" | "end" {
  if (p?.endsWith("-start") || p?.endsWith("-left")) return "start";
  if (p?.endsWith("-end") || p?.endsWith("-right")) return "end";
  return "center";
}

/**
 * Handles the `renderTarget` path: maintains internal open state so that the
 * props object passed to `renderTarget` includes a real `onClick` handler and
 * correct `isOpen` / aria values. The Radix `PopoverTrigger asChild` provides
 * the DOM anchor for positioning; the consumer's inner button opens the popover
 * via the `onClick` (aliased as `popoverClick`) received from those props.
 */
function PopoverViaRenderTarget({
  content,
  position,
  placement,
  isOpen: controlledOpen,
  onInteraction,
  onClose,
  popoverClassName,
  onOpenAutoFocus,
  onCloseAutoFocus,
  renderTarget,
}: PopoverSharedProps & {
  renderTarget: (props: Record<string, unknown>) => React.ReactNode;
}) {
  const [internalOpen, setInternalOpen] = React.useState(false);
  const controlled = controlledOpen !== undefined;
  const open = controlled ? controlledOpen! : internalOpen;

  const handleOpenChange = React.useCallback(
    (next: boolean) => {
      if (!controlled) setInternalOpen(next);
      onInteraction?.(next);
      if (!next) onClose?.();
    },
    [controlled, onInteraction, onClose],
  );

  const handleClick = React.useCallback(
    (e: React.MouseEvent) => {
      e.stopPropagation();
      handleOpenChange(!open);
    },
    [open, handleOpenChange],
  );

  // Build the trigger props the way Blueprint consumers expect:
  //   ref        — forwarded to a wrapper element so Radix can anchor the popover
  //   isOpen     — current open state
  //   onClick    — toggles the popover (consumers alias this as `popoverClick`)
  //   aria-*     — accessibility attributes
  const triggerProps: Record<string, unknown> = {
    isOpen: open,
    onClick: handleClick,
    "aria-haspopup": "dialog",
    "aria-expanded": open,
  };

  const trigger = renderTriggerChild(renderTarget(triggerProps), false);

  return (
    <UiPopover
      open={open}
      onOpenChange={handleOpenChange}
    >
      <PopoverTrigger asChild>
        {trigger}
      </PopoverTrigger>
      <PopoverContent
        side={mapPosition(position ?? placement)}
        align={mapAlignment(position ?? placement)}
        className={popoverClassName}
        onOpenAutoFocus={onOpenAutoFocus}
        onCloseAutoFocus={onCloseAutoFocus}
      >
        {content}
      </PopoverContent>
    </UiPopover>
  );
}

export function Popover({
  content,
  children,
  position,
  placement,
  disabled,
  isOpen,
  onInteraction,
  onClose,
  popoverClassName,
  onOpenAutoFocus,
  onCloseAutoFocus,
  fill,
  renderTarget,
}: {
  content?: React.ReactNode;
  children?: React.ReactNode;
  position?: string;
  placement?: string;
  minimal?: boolean;
  disabled?: boolean;
  isOpen?: boolean;
  onInteraction?: (nextOpen: boolean) => void;
  onClose?: () => void;
  className?: string;
  popoverClassName?: string;
  onOpenAutoFocus?: React.ComponentProps<typeof PopoverContent>["onOpenAutoFocus"];
  onCloseAutoFocus?: React.ComponentProps<typeof PopoverContent>["onCloseAutoFocus"];
  fill?: boolean;
  modifiers?: unknown;
  captureDismiss?: boolean;
  hasBackdrop?: boolean;
  interactionKind?: string;
  renderTarget?: (props: Record<string, unknown>) => React.ReactNode;
  autoFocus?: boolean;
  enforceFocus?: boolean;
  usePortal?: boolean;
  matchTargetWidth?: boolean;
  targetTagName?: string;
  popoverRef?: unknown;
  targetProps?: Record<string, unknown>;
}) {
  if (disabled) return <>{children}</>;

  // When a renderTarget factory is provided, delegate to the dedicated component
  // so it can maintain internal open state and pass real props to the factory.
  if (renderTarget) {
    return (
      <PopoverViaRenderTarget
        content={content}
        position={position}
        placement={placement}
        isOpen={isOpen}
        onInteraction={onInteraction}
        onClose={onClose}
        popoverClassName={popoverClassName}
        onOpenAutoFocus={onOpenAutoFocus}
        onCloseAutoFocus={onCloseAutoFocus}
        renderTarget={renderTarget}
      />
    );
  }

  // `children` path — leave uncontrolled when isOpen is undefined.
  const controlled = isOpen !== undefined;
  const trigger = renderTriggerChild(children, fill);
  return (
    <UiPopover
      {...(controlled ? { open: isOpen } : {})}
      onOpenChange={(o) => {
        onInteraction?.(o);
        if (!o) onClose?.();
      }}
    >
      <PopoverTrigger asChild>
        {trigger}
      </PopoverTrigger>
      <PopoverContent
        side={mapPosition(position ?? placement)}
        align={mapAlignment(position ?? placement)}
        className={popoverClassName}
        onOpenAutoFocus={onOpenAutoFocus}
        onCloseAutoFocus={onCloseAutoFocus}
      >
        {content}
      </PopoverContent>
    </UiPopover>
  );
}

// ───────────────────────────────────────────────────────────────────────────
// Menu / MenuItem / MenuDivider
// ───────────────────────────────────────────────────────────────────────────

export function Menu({
  children,
  className,
  style,
}: {
  children?: React.ReactNode;
  className?: string;
  style?: React.CSSProperties;
}) {
  return (
    <div
      role="menu"
      className={cn("min-w-[180px] flex flex-col gap-0.5", className)}
      style={style}
    >
      {children}
    </div>
  );
}

export function MenuItem({
  icon,
  text,
  label,
  labelElement,
  onClick,
  intent,
  disabled,
  active,
  selected,
  className,
  children,
}: {
  icon?: IconName | React.ReactElement;
  text?: React.ReactNode;
  label?: React.ReactNode;
  labelElement?: React.ReactNode;
  onClick?: React.MouseEventHandler<HTMLButtonElement>;
  intent?: Intent;
  disabled?: boolean;
  active?: boolean;
  selected?: boolean;
  className?: string;
  roleStructure?: string;
  shouldDismissPopover?: boolean;
  children?: React.ReactNode;
}) {
  return (
    <button
      type="button"
      role="menuitem"
      disabled={disabled}
      onClick={onClick}
      className={cn(
        "flex items-center gap-2 w-full text-left rounded-[var(--r-sm)] px-2 py-1.5 text-[13px] text-[var(--text-dim)] hover:bg-[var(--accent-soft)] hover:text-[var(--text)] disabled:opacity-50 disabled:pointer-events-none transition-colors",
        intent === "danger" && "text-[var(--danger)]",
        (active || selected) && "bg-[var(--accent-soft)] text-[var(--accent)]",
        className
      )}
    >
      {renderIcon(icon, 15)}
      <span className="flex-1">{text ?? children}</span>
      {label != null && (
        <span className="text-[var(--text-mute)] text-[11px]">{label}</span>
      )}
      {labelElement}
    </button>
  );
}

export function MenuDivider({
  title,
  className,
}: {
  title?: React.ReactNode;
  className?: string;
}) {
  if (title != null) {
    return (
      <div
        className={cn(
          "px-2 pt-2 pb-1 text-[10.5px] font-semibold uppercase tracking-wider text-[var(--text-mute)] font-[family-name:var(--font-mono)]",
          className
        )}
      >
        {title}
      </div>
    );
  }
  return <div className={cn("my-1 h-px bg-[var(--border)]", className)} />;
}

// ───────────────────────────────────────────────────────────────────────────
// InputGroup
// ───────────────────────────────────────────────────────────────────────────

export function InputGroup({
  leftIcon,
  leftElement,
  rightElement,
  value,
  defaultValue,
  onChange,
  onKeyDown,
  onKeyPress,
  placeholder,
  type = "text",
  disabled,
  large,
  small,
  round,
  fill,
  className,
  inputRef,
  autoFocus,
  name,
  id,
  readOnly,
  maxLength,
  "aria-label": ariaLabel,
  style,
  autoComplete,
  onBlur,
  onFocus,
  tabIndex,
  min,
  max,
}: {
  leftIcon?: IconName;
  leftElement?: React.ReactNode;
  rightElement?: React.ReactNode;
  value?: string;
  defaultValue?: string;
  onChange?: React.ChangeEventHandler<HTMLInputElement>;
  onKeyDown?: React.KeyboardEventHandler<HTMLInputElement>;
  onKeyPress?: React.KeyboardEventHandler<HTMLInputElement>;
  placeholder?: string;
  type?: string;
  disabled?: boolean;
  large?: boolean;
  small?: boolean;
  round?: boolean;
  fill?: boolean;
  className?: string;
  inputRef?: React.Ref<HTMLInputElement>;
  autoFocus?: boolean;
  name?: string;
  id?: string;
  intent?: Intent;
  readOnly?: boolean;
  maxLength?: number;
  "aria-label"?: string;
  style?: React.CSSProperties;
  autoComplete?: string;
  onBlur?: React.FocusEventHandler<HTMLInputElement>;
  onFocus?: React.FocusEventHandler<HTMLInputElement>;
  tabIndex?: number;
  min?: number;
  max?: number;
}) {
  const hasLeft = leftIcon != null || leftElement != null;
  return (
    <div className={cn("relative flex items-center", fill && "w-full", className)}>
      {leftIcon != null && (
        <Icon
          icon={leftIcon}
          size={15}
          className="pointer-events-none absolute left-2.5 text-[var(--text-mute)]"
        />
      )}
      {leftElement != null && (
        <span className="absolute left-2 flex items-center">{leftElement}</span>
      )}
      <input
        ref={inputRef}
        type={type}
        value={value}
        defaultValue={defaultValue}
        onChange={onChange}
        onKeyDown={onKeyDown}
        onKeyPress={onKeyPress}
        placeholder={placeholder}
        disabled={disabled}
        autoFocus={autoFocus}
        name={name}
        id={id}
        readOnly={readOnly}
        maxLength={maxLength}
        aria-label={ariaLabel}
        style={style}
        autoComplete={autoComplete}
        onBlur={onBlur}
        onFocus={onFocus}
        tabIndex={tabIndex}
        min={min}
        max={max}
        className={cn(
          "w-full rounded-[var(--r)] border border-[var(--border)] bg-[var(--surface-2)] px-3 text-[13px] text-[var(--text)] placeholder:text-[var(--text-mute)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-line)] focus-visible:border-[var(--accent-line)] transition-colors duration-150 disabled:opacity-50 disabled:pointer-events-none",
          large ? "h-10" : small ? "h-8" : "h-9",
          hasLeft && "pl-8",
          rightElement != null && "pr-9",
          round && "rounded-full"
        )}
      />
      {rightElement != null && (
        <span className="absolute right-1.5 flex items-center">{rightElement}</span>
      )}
    </div>
  );
}

// ───────────────────────────────────────────────────────────────────────────
// TextArea
// ───────────────────────────────────────────────────────────────────────────

export function TextArea({
  value,
  defaultValue,
  onChange,
  placeholder,
  fill,
  rows,
  large,
  growVertically,
  className,
  disabled,
  inputRef,
  id,
  name,
  maxLength,
  "aria-label": ariaLabel,
  onKeyDown,
  style,
}: {
  value?: string;
  defaultValue?: string;
  onChange?: React.ChangeEventHandler<HTMLTextAreaElement>;
  placeholder?: string;
  fill?: boolean;
  rows?: number;
  large?: boolean;
  growVertically?: boolean;
  className?: string;
  disabled?: boolean;
  inputRef?: React.Ref<HTMLTextAreaElement>;
  id?: string;
  name?: string;
  maxLength?: number;
  "aria-label"?: string;
  onKeyDown?: React.KeyboardEventHandler<HTMLTextAreaElement>;
  style?: React.CSSProperties;
}) {
  return (
    <textarea
      ref={inputRef}
      value={value}
      defaultValue={defaultValue}
      onChange={onChange}
      onKeyDown={onKeyDown}
      placeholder={placeholder}
      rows={rows}
      disabled={disabled}
      id={id}
      name={name}
      maxLength={maxLength}
      aria-label={ariaLabel}
      style={style}
      className={cn(
        "rounded-[var(--r)] border border-[var(--border)] bg-[var(--surface-2)] p-2 text-[13px] text-[var(--text)] placeholder:text-[var(--text-mute)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-line)] focus-visible:border-[var(--accent-line)] transition-colors duration-150 disabled:opacity-50 disabled:pointer-events-none",
        large && "text-sm",
        growVertically && "resize-y",
        fill && "w-full",
        className
      )}
    />
  );
}

// ───────────────────────────────────────────────────────────────────────────
// FormGroup
// ───────────────────────────────────────────────────────────────────────────

export function FormGroup({
  label,
  labelFor,
  labelInfo,
  helperText,
  inline,
  className,
  style,
  children,
}: {
  label?: React.ReactNode;
  labelFor?: string;
  labelInfo?: React.ReactNode;
  helperText?: React.ReactNode;
  inline?: boolean;
  disabled?: boolean;
  className?: string;
  children?: React.ReactNode;
  intent?: Intent;
  style?: React.CSSProperties;
}) {
  return (
    <div
      style={style}
      className={cn(
        "flex flex-col gap-1.5",
        inline && "flex-row items-center gap-2",
        className
      )}
    >
      {label != null && (
        <label
          htmlFor={labelFor}
          className="text-[12.5px] font-medium text-[var(--text-dim)]"
        >
          {label}
          {labelInfo != null && (
            <span className="text-[var(--text-mute)] ml-1">{labelInfo}</span>
          )}
        </label>
      )}
      {children}
      {helperText != null && (
        <div className="text-[11px] text-[var(--text-mute)]">{helperText}</div>
      )}
    </div>
  );
}

// ───────────────────────────────────────────────────────────────────────────
// HTMLSelect
// ───────────────────────────────────────────────────────────────────────────

type SelectOption =
  | string
  | number
  | { label: string; value: string | number; disabled?: boolean };

export function HTMLSelect({
  options,
  value,
  defaultValue,
  onChange,
  fill,
  disabled,
  className,
  id,
  name,
  style,
  onKeyDown,
  children,
}: {
  options?: SelectOption[];
  value?: string | number;
  defaultValue?: string | number;
  onChange?: (e: React.ChangeEvent<HTMLSelectElement>) => void;
  fill?: boolean;
  large?: boolean;
  minimal?: boolean;
  disabled?: boolean;
  className?: string;
  iconName?: IconName;
  id?: string;
  name?: string;
  style?: React.CSSProperties;
  onKeyDown?: React.KeyboardEventHandler<HTMLSelectElement>;
  children?: React.ReactNode;
}) {
  return (
    <div style={style} className={cn("relative inline-flex items-center", fill && "w-full", className)}>
      <select
        value={value}
        defaultValue={defaultValue}
        onChange={onChange}
        onKeyDown={onKeyDown}
        disabled={disabled}
        id={id}
        name={name}
        className={cn(
          "h-9 rounded-[var(--r)] border border-[var(--border)] bg-[var(--surface-2)] pl-3 pr-8 text-[13px] text-[var(--text)] appearance-none focus:outline-none focus:ring-2 focus:ring-[var(--accent-line)] disabled:opacity-50 disabled:pointer-events-none",
          fill && "w-full"
        )}
      >
        {children ??
          options?.map((o) =>
            typeof o === "object" ? (
              <option key={o.value} value={o.value} disabled={o.disabled}>
                {o.label}
              </option>
            ) : (
              <option key={o} value={o}>
                {o}
              </option>
            )
          )}
      </select>
      <ChevronDown
        size={15}
        className="pointer-events-none absolute right-2 text-[var(--text-mute)]"
      />
    </div>
  );
}

// ───────────────────────────────────────────────────────────────────────────
// NumericInput
// ───────────────────────────────────────────────────────────────────────────

export function NumericInput({
  value,
  defaultValue,
  onValueChange,
  min,
  max,
  stepSize,
  disabled,
  fill,
  large,
  leftIcon,
  className,
  id,
  placeholder,
  style,
  small,
}: {
  value?: number | string;
  defaultValue?: number | string;
  onValueChange?: (valueAsNumber: number, valueAsString: string) => void;
  min?: number;
  max?: number;
  stepSize?: number;
  majorStepSize?: number;
  minorStepSize?: number;
  disabled?: boolean;
  fill?: boolean;
  large?: boolean;
  buttonPosition?: string;
  leftIcon?: IconName;
  className?: string;
  id?: string;
  placeholder?: string;
  allowNumericCharactersOnly?: boolean;
  clampValueOnBlur?: boolean;
  style?: React.CSSProperties;
  small?: boolean;
}) {
  return (
    <div className={cn("relative flex items-center", fill && "w-full", className)}>
      {leftIcon != null && (
        <Icon
          icon={leftIcon}
          size={15}
          className="pointer-events-none absolute left-2.5 text-[var(--text-mute)]"
        />
      )}
      <input
        type="number"
        style={style}
        value={value}
        defaultValue={defaultValue}
        min={min}
        max={max}
        step={stepSize}
        disabled={disabled}
        id={id}
        placeholder={placeholder}
        onChange={(e) => onValueChange?.(Number(e.target.value), e.target.value)}
        className={cn(
          "w-full rounded-[var(--r)] border border-[var(--border)] bg-[var(--surface-2)] px-3 text-[13px] text-[var(--text)] placeholder:text-[var(--text-mute)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-line)] focus-visible:border-[var(--accent-line)] transition-colors duration-150 disabled:opacity-50 disabled:pointer-events-none",
          large ? "h-10" : small ? "h-8" : "h-9",
          leftIcon != null && "pl-8",
          fill && "w-full"
        )}
      />
    </div>
  );
}

// ───────────────────────────────────────────────────────────────────────────
// Card
// ───────────────────────────────────────────────────────────────────────────

export function Card({
  interactive,
  elevation,
  onClick,
  className,
  children,
  style,
}: {
  interactive?: boolean;
  elevation?: number;
  onClick?: React.MouseEventHandler;
  className?: string;
  children?: React.ReactNode;
  style?: React.CSSProperties;
}) {
  return (
    <div
      onClick={onClick}
      style={style}
      className={cn(
        "rounded-[var(--r-lg)] border border-[var(--border)] bg-[var(--surface)] p-[var(--pad)] text-[var(--text)]",
        elevation != null && elevation > 1 && "shadow-[var(--shadow)]",
        interactive &&
          "cursor-pointer hover:border-[var(--border-strong)] transition-colors",
        className
      )}
    >
      {children}
    </div>
  );
}

// ───────────────────────────────────────────────────────────────────────────
// Collapse
// ───────────────────────────────────────────────────────────────────────────

export function Collapse({
  isOpen,
  keepChildrenMounted,
  children,
  className,
}: {
  isOpen?: boolean;
  keepChildrenMounted?: boolean;
  children?: React.ReactNode;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "grid transition-[grid-template-rows] duration-200 [transition-timing-function:var(--ease)]",
        isOpen ? "grid-rows-[1fr]" : "grid-rows-[0fr]",
        className
      )}
    >
      <div className="overflow-hidden min-h-0">
        {isOpen || keepChildrenMounted ? children : null}
      </div>
    </div>
  );
}

// ───────────────────────────────────────────────────────────────────────────
// Tabs / Tab
// ───────────────────────────────────────────────────────────────────────────

export interface TabProps {
  id: string;
  title?: React.ReactNode;
  panel?: React.ReactNode;
  disabled?: boolean;
  icon?: IconName;
  tagContent?: React.ReactNode;
  className?: string;
  children?: React.ReactNode;
}

/** Tab is a config marker; its props are read by <Tabs>. Renders nothing. */
export function Tab(_props: TabProps): React.ReactElement | null {
  return null;
}

export function Tabs({
  selectedTabId,
  onChange,
  children,
  className,
  vertical,
}: {
  id?: string;
  selectedTabId?: string;
  onChange?: (newId: string, prevId: string | undefined, e: React.MouseEvent) => void;
  children?: React.ReactNode;
  className?: string;
  large?: boolean;
  vertical?: boolean;
  renderActiveTabPanelOnly?: boolean;
  animate?: boolean;
}) {
  const tabs = React.Children.toArray(children).filter(
    (c): c is React.ReactElement<TabProps> =>
      React.isValidElement(c) && (c.props as TabProps).id !== undefined
  );

  const active =
    tabs.find((t) => t.props.id === selectedTabId) ?? tabs[0];

  // TODO(motion): Sliding-pill indicator — the active tab currently teleports
  // (border-color switches instantly per tab). The chosen treatment is a moving
  // pill that translates via framer-motion layoutId. Implementing it here would
  // require converting the tablist to a positioned container + a motion.div
  // indicator, which is a structural rewrite with meaningful risk of breaking
  // the 5+ call-sites that pass custom classNames or vertical=true.
  // Prefer doing this in the canonical src/components/ui/tabs.tsx (Radix Tabs)
  // where the primitives already use layoutId patterns (see sidebar indicator).
  // When that primitive is updated, swap this shim's Tabs to delegate to it.

  return (
    <div className={cn(vertical && "flex gap-4", className)}>
      <div
        role="tablist"
        className={cn(
          "inline-flex gap-1 border-[var(--border)]",
          vertical ? "flex-col border-r pr-2" : "border-b"
        )}
      >
        {tabs.map((t) => {
          const isActive = active != null && t.props.id === active.props.id;
          return (
            <button
              key={t.props.id}
              type="button"
              role="tab"
              disabled={t.props.disabled}
              data-active={isActive ? "" : undefined}
              onClick={(e) => onChange?.(t.props.id, selectedTabId, e)}
              className={cn(
                "px-3 py-2 text-[13px] text-[var(--text-dim)] border-b-2 border-transparent transition-colors hover:text-[var(--text)] disabled:opacity-50 disabled:pointer-events-none inline-flex items-center gap-1.5",
                isActive && "text-[var(--accent)] border-[var(--accent)]"
              )}
            >
              {t.props.icon != null && <Icon icon={t.props.icon} size={14} />}
              {t.props.title}
              {t.props.tagContent}
            </button>
          );
        })}
      </div>
      {active != null && (
        <div role="tabpanel" className="flex-1">
          {active.props.panel ?? active.props.children}
        </div>
      )}
    </div>
  );
}

// ───────────────────────────────────────────────────────────────────────────
// NonIdealState
// ───────────────────────────────────────────────────────────────────────────

export function NonIdealState({
  icon,
  title,
  description,
  action,
  children,
  className,
}: {
  icon?: IconName | React.ReactElement;
  title?: React.ReactNode;
  description?: React.ReactNode;
  action?: React.ReactNode;
  children?: React.ReactNode;
  className?: string;
  layout?: string;
}) {
  return (
    <div
      className={cn(
        "flex flex-col items-center justify-center gap-3 py-12 text-center",
        className
      )}
    >
      {icon != null &&
        (typeof icon === "string" ? (
          <div className="grid h-12 w-12 place-items-center rounded-full border border-[var(--border)] bg-[var(--surface-2)] text-[var(--text-mute)]">
            <Icon icon={icon} size={22} />
          </div>
        ) : (
          icon
        ))}
      {title != null && (
        <div className="text-[15px] font-semibold text-[var(--text)]">{title}</div>
      )}
      {description != null && (
        <div className="max-w-[320px] text-[13px] text-[var(--text-dim)]">
          {description}
        </div>
      )}
      {action}
      {children}
    </div>
  );
}

// ───────────────────────────────────────────────────────────────────────────
// ProgressBar
// ───────────────────────────────────────────────────────────────────────────

export function ProgressBar({
  value,
  intent,
  className,
}: {
  value?: number;
  intent?: Intent;
  stripes?: boolean;
  animate?: boolean;
  className?: string;
}) {
  const indeterminate = value == null;
  const color = intentColor(intent);
  return (
    <div
      className={cn(
        "h-1.5 w-full rounded-full bg-[var(--surface-3)] overflow-hidden",
        className
      )}
    >
      <div
        className={cn("h-full rounded-full transition-all", indeterminate && "animate-pulse")}
        style={{
          width: indeterminate ? "100%" : `${Math.round((value ?? 0) * 100)}%`,
          background: color,
        }}
      />
    </div>
  );
}

// ───────────────────────────────────────────────────────────────────────────
// H5 / Text
// ───────────────────────────────────────────────────────────────────────────

export function H5({
  className,
  children,
  ...props
}: React.HTMLAttributes<HTMLHeadingElement>) {
  return (
    <h5
      className={cn(
        "font-[family-name:var(--font-display)] text-[15px] font-semibold text-[var(--text)]",
        className
      )}
      {...props}
    >
      {children}
    </h5>
  );
}

export function Text({
  children,
  className,
  ellipsize,
}: {
  children?: React.ReactNode;
  className?: string;
  ellipsize?: boolean;
  tagName?: string;
}) {
  return (
    <span className={cn(ellipsize && "truncate block", className)}>{children}</span>
  );
}

// ───────────────────────────────────────────────────────────────────────────
// Slider — Blueprint's value slider (single value, onChange/onRelease).
// ───────────────────────────────────────────────────────────────────────────

export function Slider({
  min = 0,
  max = 10,
  stepSize = 1,
  value,
  onChange,
  onRelease,
  disabled,
  className,
}: {
  min?: number;
  max?: number;
  stepSize?: number;
  labelStepSize?: number;
  labelRenderer?: ((value: number) => React.ReactNode) | boolean;
  value?: number;
  onChange?: (value: number) => void;
  onRelease?: (value: number) => void;
  disabled?: boolean;
  vertical?: boolean;
  className?: string;
  showTrackFill?: boolean;
}) {
  return (
    <UiSlider
      min={min}
      max={max}
      step={stepSize}
      value={value != null ? [value] : undefined}
      disabled={disabled}
      onValueChange={(v: number[]) => onChange?.(v[0] ?? 0)}
      onValueCommit={(v: number[]) => onRelease?.(v[0] ?? 0)}
      className={className}
    />
  );
}

// ───────────────────────────────────────────────────────────────────────────
// HTMLTable
// ───────────────────────────────────────────────────────────────────────────

export function HTMLTable({
  striped,
  bordered,
  interactive,
  compact,
  className,
  children,
  ...rest
}: React.TableHTMLAttributes<HTMLTableElement> & {
  striped?: boolean;
  bordered?: boolean;
  interactive?: boolean;
  compact?: boolean;
}) {
  return (
    <table
      className={cn(
        "w-full border-collapse text-[13px] text-[var(--text)]",
        bordered && "[&_td]:border [&_th]:border [&_td]:border-[var(--border)] [&_th]:border-[var(--border)]",
        striped && "[&_tbody_tr:nth-child(even)]:bg-[var(--surface-2)]",
        interactive && "[&_tbody_tr:hover]:bg-[var(--surface-2)]",
        compact
          ? "[&_td]:px-2 [&_td]:py-1 [&_th]:px-2 [&_th]:py-1"
          : "[&_td]:px-3 [&_td]:py-2 [&_th]:px-3 [&_th]:py-2",
        "[&_th]:text-left [&_th]:font-[family-name:var(--font-mono)] [&_th]:text-[10.5px] [&_th]:uppercase [&_th]:tracking-wider [&_th]:text-[var(--text-mute)]",
        className,
      )}
      {...rest}
    >
      {children}
    </table>
  );
}
