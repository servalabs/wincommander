import type { AppSettings } from "../types/settings";

const DEFAULT_COMPANY_NAME = "ServaLabs";
const DEFAULT_PRODUCT_NAME = "WinCommander";

function normalizeName(value: string | null | undefined, fallback: string): string {
  const trimmed = value?.trim();
  return trimmed && trimmed.length > 0 ? trimmed : fallback;
}

export function getDisplayBranding(appSettings: AppSettings | null | undefined) {
  const branding = appSettings?.ideal?.identity?.branding;
  const companyName = normalizeName(branding?.companyName, DEFAULT_COMPANY_NAME);
  const productName = normalizeName(branding?.productName, DEFAULT_PRODUCT_NAME);

  return {
    companyName,
    productName,
    companyLabel: companyName.toUpperCase(),
    productLabel: productName.toUpperCase(),
    titleLabel: [companyName, productName].filter(Boolean).join(" ").toUpperCase(),
  };
}