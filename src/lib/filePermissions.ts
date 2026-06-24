export interface RwxTriplet {
  r: boolean;
  w: boolean;
  x: boolean;
}

/** Split an octal mode into [special, owner, group, other], where `special`
 *  carries the setuid(4)/setgid(2)/sticky(1) bits. */
export function octalToTriplets(octal: string): [number, number, number, number] {
  const s = octal.replace(/[^0-7]/g, "").padStart(4, "0").slice(-4);
  return [
    parseInt(s[0] ?? "0", 10),
    parseInt(s[1] ?? "0", 10),
    parseInt(s[2] ?? "0", 10),
    parseInt(s[3] ?? "0", 10),
  ];
}

/** Recombine the special + rwx triplets into an octal string. The special digit
 *  is only prefixed when non-zero, so ordinary modes stay 3 digits. */
export function tripletsToOctal(
  special: number,
  owner: number,
  group: number,
  other: number,
): string {
  const base = `${owner}${group}${other}`;
  return special > 0 ? `${special}${base}` : base;
}

export function bitsToRwx(bits: number): RwxTriplet {
  return {
    r: (bits & 4) !== 0,
    w: (bits & 2) !== 0,
    x: (bits & 1) !== 0,
  };
}

export function rwxToBits(t: RwxTriplet): number {
  return (t.r ? 4 : 0) + (t.w ? 2 : 0) + (t.x ? 1 : 0);
}

export function parseModeInput(input: string): string | null {
  const t = input.trim();
  if (!/^[0-7]{1,4}$/.test(t)) return null;
  // Preserve a leading special digit (4 chars); otherwise normalize to 3.
  return t.length === 4 ? t : t.padStart(3, "0");
}
