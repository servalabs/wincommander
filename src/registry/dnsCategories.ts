// Single source of truth for the ControlD "Simple Firewall" content
// categories. Previously duplicated (and drifted) between the Network panel
// (CONTROLD_CATEGORIES) and Help & Setup (DNS_CATEGORIES) — ids must match
// the ControlD DoH slug fragments exactly (`no-<id>-<id>...`).
import type { IconName } from "@/components/ui/bp";

export interface DnsCategoryDef {
    id: string;
    label: string;
    /** Longer description used by the Network panel's category cards. */
    description: string;
    /** Short concrete example used by Help & Setup toggle cards. */
    example: string;
    icon: IconName;
}

export const DNS_CATEGORIES: DnsCategoryDef[] = [
    { id: 'ads', label: 'Ads & Trackers', description: 'Stops adverts and the invisible code that watches what you do online.', example: 'e.g. banner / video ads, Google Analytics', icon: 'eye-off' },
    { id: 'porn', label: 'Adult Content', description: 'Blocks porn and explicit websites.', example: 'e.g. Pornhub, OnlyFans, explicit sites', icon: 'ban-circle' },
    { id: 'dating', label: 'Dating', description: 'Hides dating sites and apps.', example: 'e.g. Tinder, Bumble, match.com', icon: 'people' },
    { id: 'drugs', label: 'Drugs', description: 'Blocks sites that sell alcohol, tobacco, vapes, and recreational drugs.', example: 'e.g. alcohol, tobacco & vape shops', icon: 'pill' },
    { id: 'gambling', label: 'Gambling', description: 'Stops casino, betting, and lottery sites.', example: 'e.g. Bet365, casinos, lottery sites', icon: 'dollar' },
    { id: 'gov', label: 'Government', description: 'Hides government websites (you can still pay taxes — niche feature).', example: 'e.g. tax, passport & gov portals', icon: 'office' },
    { id: 'malware', label: 'Malware', description: 'Blocks sites that try to plant viruses. Leave this on.', example: 'e.g. virus & drive-by-download sites', icon: 'shield-alert' },
    { id: 'phishing', label: 'Phishing', description: 'Blocks fake bank / Google / etc. sites that try to steal passwords.', example: 'e.g. fake bank & Google login pages', icon: 'key' },
    { id: 'social', label: 'Social Media', description: 'Blocks Facebook, Instagram, X, TikTok, and their trackers.', example: 'e.g. Facebook, Instagram, X, TikTok', icon: 'chat' },
];

/** Category ids auto-enabled when the main Encrypted DNS toggle turns on
 *  with no categories selected yet — everything except gov + social, which
 *  are niche/opt-in blocks rather than security defaults. */
export const DNS_CATEGORY_DEFAULT_IDS: string[] = DNS_CATEGORIES
    .map((c) => c.id)
    .filter((id) => id !== 'gov' && id !== 'social');

/** Build the ControlD DoH hostname slug from a selected category set. Fixed
 *  order so the same selection always produces the same URL. Empty → "". */
export function buildControldSlug(selected: Set<string>): string {
    if (selected.size === 0) return "";
    const order = DNS_CATEGORIES.map((c) => c.id);
    const picked = order.filter((id) => selected.has(id));
    return `no-${picked.join('-')}`;
}

/** Parse a ControlD DoH slug back into the selected category id set. */
export function parseControldSlug(slug: string | null | undefined): Set<string> {
    if (!slug) return new Set();
    const m = /^no-(.+)$/.exec(slug);
    if (!m) return new Set();
    const validIds = new Set(DNS_CATEGORIES.map((c) => c.id));
    return new Set(m[1].split('-').filter((id) => validIds.has(id)));
}
