import type { AppLicenseStatus } from "../hooks/useBackend";

const PLAN_LABELS: Record<string, string> = {
  pro_lifetime: "Pro Lifetime",
  pro_membership: "Pro Membership",
  investigator: "Investigator",
  fleet: "Fleet",
  trial: "Free Trial",
  paid: "Pro",
  pro: "Pro",
  all: "Pro",
};

const SERVICE_LABELS: Record<string, string> = {
  advanced: "Investigator Mode",
  fleet: "Fleet management",
  netwall: "Netwall",
};

export function licensePlanLabel(plan?: string | null): string {
  if (!plan) return "No paid plan";
  return PLAN_LABELS[plan.toLowerCase()] ?? plan.replaceAll("_", " ");
}

export function activeLicenseServices(status?: AppLicenseStatus | null): string[] {
  const features = status?.active_service_features ?? status?.features ?? [];
  return features
    .filter((feature) => SERVICE_LABELS[feature])
    .map((feature) => SERVICE_LABELS[feature]);
}

export function hasRetainedPro(status?: AppLicenseStatus | null): boolean {
  const base = status?.base_features ?? status?.features ?? [];
  return base.includes("paid");
}

export function licenseAccessSummary(status?: AppLicenseStatus | null): string | null {
  if (!status?.licensed || status.trial_active) return null;
  const plan = status.plan?.toLowerCase();
  const services = activeLicenseServices(status);

  if (plan === "pro_lifetime") {
    return "Normal Pro and Pro updates are yours for life.";
  }
  if (plan === "pro_membership") {
    return services.includes("Netwall")
      ? "Pro updates and hosted Netwall are active. If membership ends, your last eligible normal-Pro build remains usable."
      : "Membership services have ended; your last eligible normal-Pro build remains usable.";
  }
  if (plan === "investigator") {
    return services.includes("Investigator Mode")
      ? "Investigator Mode and hosted Netwall are active. Existing cases remain readable and exportable if the term ends."
      : "Investigator and Netwall access have ended; existing cases remain readable and normal Pro remains usable.";
  }
  if (plan === "fleet") {
    return services.includes("Fleet management")
      ? "Fleet management and hosted Netwall are active. Each paid seat covers one managed endpoint."
      : "Fleet is outside its active term. Management becomes read-only; policies and customer data are not deleted.";
  }
  return hasRetainedPro(status) ? "Normal Pro is available on this device." : null;
}

export function licenseDeviceSummary(status?: AppLicenseStatus | null): string {
  if (status?.plan?.toLowerCase() === "fleet") {
    if (status.seat_limit == null) return "One managed Windows endpoint per Fleet seat.";
    return `${status.seats_used ?? "?"} of ${status.seat_limit} Fleet seats in use`;
  }
  const limit = status?.seat_limit ?? 3;
  return `${status?.seats_used ?? "?"} of ${limit} transferable device slots active`;
}

export function licenseStateLabel(status?: AppLicenseStatus | null): string {
  if (!status) return "UNKNOWN";
  if (!status.configured) return "UNCONFIGURED";
  if (status.trial_active) return "FREE TRIAL";
  if (!status.licensed) return "NOT ACTIVE";
  if (!status.valid) return "EXPIRED";

  const plan = status.plan?.toLowerCase();
  const services = status.active_service_features ?? status.features ?? [];
  if (plan === "fleet" && !services.includes("fleet")) return "READ-ONLY";
  if (plan === "pro_membership" && !services.includes("netwall")) return "PRO RETAINED";
  if (plan === "investigator" && !services.includes("advanced")) return "PRO RETAINED";
  return "ACTIVE";
}
