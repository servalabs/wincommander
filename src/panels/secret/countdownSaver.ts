/** Coalesce slider writes, while preserving the last choice on navigation. */
export function createCountdownSaver(save: (seconds: number) => void, delay = 250) {
    let pending: number | null = null;
    let timer: ReturnType<typeof setTimeout> | null = null;
    const flush = () => {
        if (timer) clearTimeout(timer);
        timer = null;
        if (pending !== null) {
            const value = pending;
            pending = null;
            save(value);
        }
    };
    return {
        queue(value: number) {
            pending = value;
            if (timer) clearTimeout(timer);
            timer = setTimeout(flush, delay);
        },
        flush,
    };
}
