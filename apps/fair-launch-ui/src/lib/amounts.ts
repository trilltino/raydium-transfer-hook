/** Parse a decimal string like "1.25" into base units. Returns null for anything that is not a plain, non-negative amount that fits. */
export function parseAmount(text: string, decimals: number): bigint | null {
  const trimmed = text.trim().replace(/,/g, '');
  if (!/^\d*\.?\d*$/.test(trimmed) || trimmed === '' || trimmed === '.') return null;
  const [whole, fraction = ''] = trimmed.split('.');
  if (fraction.length > decimals) return null;
  const raw = BigInt(`${whole || '0'}${fraction.padEnd(decimals, '0')}`);
  return raw < 1n << 64n ? raw : null;
}

/** Format base units with thousands separators and at most `maxFraction` decimals (trailing zeros trimmed). */
export function formatAmount(raw: bigint, decimals: number, maxFraction = 6): string {
  const negative = raw < 0n;
  const abs = negative ? -raw : raw;
  const base = 10n ** BigInt(decimals);
  const whole = abs / base;
  let fraction = (abs % base).toString().padStart(decimals, '0').slice(0, maxFraction).replace(/0+$/, '');
  if (decimals === 0) fraction = '';
  const grouped = whole.toString().replace(/\B(?=(\d{3})+(?!\d))/g, ',');
  return `${negative ? '-' : ''}${grouped}${fraction ? `.${fraction}` : ''}`;
}

/** The plain, unseparated decimal string an input box should hold (for Max / 50%). */
export function toInputText(raw: bigint, decimals: number): string {
  return formatAmount(raw, decimals, decimals).replace(/,/g, '');
}

export function shortKey(key: string): string {
  return key.length > 12 ? `${key.slice(0, 4)}…${key.slice(-4)}` : key;
}
