export function enabledRowsFirst<T>(rows: readonly T[], isEnabled: (row: T) => boolean): T[] {
    return rows
        .map((row, index) => ({ row, index, enabled: isEnabled(row) }))
        .sort((a, b) => {
            if (a.enabled !== b.enabled) return a.enabled ? -1 : 1;
            return a.index - b.index;
        })
        .map(({ row }) => row);
}
