// src/panels/privacy/pinRules.ts
// Calculator PIN rules, shared by the PIN editor and the Stealth mode switch.

export function validatePinInput(pin: string): string | null {
  if (pin.length < 4) return "PIN must be at least 4 digits";
  if (!/^[0-9]+$/.test(pin)) return "PIN must contain only digits — the calculator can't display letters";
  if (pin.startsWith("0")) return "PIN can't start with 0 — the calculator drops leading zeros";
  if (/^(\d)\1+$/.test(pin)) return "PIN can't use the same digit repeated";

  const digits = [...pin].map((d) => Number(d));
  const ascending = digits.every((d, i) => i === 0 || d === digits[i - 1] + 1);
  const descending = digits.every((d, i) => i === 0 || d === digits[i - 1] - 1);
  if (ascending || descending) return "PIN can't be a simple ascending or descending sequence";

  return null;
}
