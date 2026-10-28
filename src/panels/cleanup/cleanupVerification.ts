export type CleanupVerificationStatus =
  | 'cleared' | 'reduced' | 'reset' | 'unchanged' | 'deferred' | 'failed' | 'unverified';

export type CleanupVerification = {
  status: CleanupVerificationStatus;
  verifiedAt: string;
};

export function createCleanupVerification(
  status: CleanupVerificationStatus,
  verifiedAt = new Date().toISOString(),
): CleanupVerification {
  return { status, verifiedAt };
}

export function retainCleanupVerification<T extends { verification?: CleanupVerification }>(
  next: T,
  previous?: { verification?: CleanupVerification },
): T {
  return next.verification || !previous?.verification
    ? next
    : { ...next, verification: previous.verification };
}
