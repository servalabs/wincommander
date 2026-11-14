import type { DuplicateFile } from "../../hooks/useBackend";

export function duplicateProfileKey(file: DuplicateFile): string {
  if ((file.profileScope === "current" || file.profileScope === "other") && file.profileRoot) {
    return `${file.profileScope}:${file.profileRoot}`;
  }
  return file.profileScope === "outside" ? "outside" : "unavailable";
}

export function duplicateProfileLabel(file: DuplicateFile): string {
  if (file.profileScope === "current" && file.profileRoot) return `Current user · ${file.profileRoot}`;
  if (file.profileScope === "other" && file.profileRoot) return `Other user · ${file.profileRoot}`;
  return file.profileScope === "outside" ? "Outside registered profiles" : "Profile unavailable";
}

export function isDuplicateGroupsFiltered(query: string, category: string, profile: string): boolean {
  return query.trim().length > 0 || category !== "all" || profile !== "all";
}
