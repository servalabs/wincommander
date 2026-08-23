// src/panels/vault/mountScope.ts
//
// Pure resolution of the user's mount-scope preference into the concrete
// scope the engine actually mounts with. The two engine scopes are genuinely
// different resources, not a display preference: machine scope allocates a
// mount-manager drive letter (visible to every logged-in user), per-user
// scope links a DefineDosDevice symlink into just the calling logon session
// (see EncVolMountScope in EncVolFormatSdk.h). "auto" needs an OS hint to
// pick the sane default per machine type.

export type MountScopePreference = "auto" | "machine" | "per-user";
export type EngineMountScope = "machine" | "per-user";

/** Windows Server and Enterprise/Education "multi-session" SKUs (Azure
 *  Virtual Desktop) are the machines where more than one person is routinely
 *  signed in at once — a machine-wide drive letter is the wrong default
 *  there, so "auto" resolves to per-user. A desktop can also have a second
 *  interactive or RDP session, so unknown and client SKUs fail closed to the
 *  calling logon session. Machine scope requires an explicit choice. */
export function resolveEffectiveMountScope(
  preference: MountScopePreference | null | undefined,
  osName: string | null | undefined,
): EngineMountScope {
  if (preference === "machine" || preference === "per-user") return preference;
  void osName;
  return "per-user";
}
