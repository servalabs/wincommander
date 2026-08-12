// src/panels/privacy/removeUsersUtils.ts
//
// Pure helpers for the "Remove Users & Wipe Data" selection UI
// (RemoveUsersSection.tsx). Kept separate + tested because the
// selectability rule (never let an admin select a built-in or the
// currently-signed-in account) is the one thing that must never
// regress here — Pro re-validates server-side too, but the picker
// should never even offer an unsafe choice.

import type { LocalLoginUser } from "../../components/tweaks/managers/localUsersManagerUtils";

/** A row may be selected for removal only if it's a normal, non-current
 *  account. Built-in accounts (Administrator, Guest, DefaultAccount, …)
 *  and the account that's currently signed in are always excluded —
 *  mirrors the skip rules re-enforced server-side by the Pro sidecar. */
export function isUserSelectable(user: LocalLoginUser): boolean {
  return !user.builtIn && !user.currentUser;
}

/** Returns the full next array with `name` toggled in/out — never a
 *  delta. Settings patches deep-merge objects but replace arrays
 *  wholesale, so every write must carry the complete desired list. */
export function toggleUser(current: string[], name: string): string[] {
  return current.includes(name)
    ? current.filter((n) => n !== name)
    : [...current, name];
}
