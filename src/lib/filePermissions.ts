export interface RwxTriplet {
  r: boolean;
  w: boolean;
  x: boolean;
}

export function octalToTriplets(octal: string): [number, number, number] {
  const s = octal.replace(/^0+/, "").padStart(3, "0").slice(-3);
  return [parseInt(s[0] ?? "0", 10), parseInt(s[1] ?? "0", 10), parseInt(s[2] ?? "0", 10)];
}

export function tripletsToOctal(owner: number, group: number, other: number): string {
  return `${owner}${group}${other}`;
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
  return t.replace(/^0+/, "").padStart(3, "0").slice(-3);
}
