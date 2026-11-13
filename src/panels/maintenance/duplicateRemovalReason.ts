/** Render only known backend codes, never an arbitrary backend-provided message. */
export function duplicateRemovalReason(code: string | null | undefined): string {
  switch (code) {
    case "protected_location": return "Windows and application locations are protected.";
    case "protected_type": return "Application, executable and codec files are protected.";
    case "application_data": return "Application data is protected.";
    case "git_metadata": return "Files inside .git directories are protected.";
    case "other_profile": return "Files in another Windows user's profile are protected.";
    case "profiles_unavailable": return "Windows user profiles could not be verified.";
    default: return "This copy cannot be verified for removal. Scan again or update WinCommander.";
  }
}
