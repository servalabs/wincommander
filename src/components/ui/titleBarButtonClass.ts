// src/components/ui/titleBarButtonClass.ts
//
// Single source for the chromeless-titlebar icon-button look (bell/help/
// theme/window controls). Used to be copy-pasted as a literal string in both
// TitleBar.tsx and NotificationsMenu.tsx; centralised here so AlertsMenu and
// ProcessesMenu (the bell's Alerts/Processes split) don't add a third copy.
export const TITLEBAR_ICON_BTN =
  "grid place-items-center w-8 h-8 rounded-[var(--r-sm)] text-[var(--text-mute)] hover:text-[var(--text)] hover:bg-[var(--surface-2)] transition-colors duration-150";
