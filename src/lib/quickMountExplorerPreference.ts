export async function persistQuickMountExplorerPreference(
  enabled: boolean, write: (enabled: boolean) => Promise<void>, read: () => Promise<boolean | undefined>,
): Promise<void> {
  await write(enabled);
  if (await read() !== enabled) throw new Error("Quick Mount File Explorer preference could not be verified");
}
