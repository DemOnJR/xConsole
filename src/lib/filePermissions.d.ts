export interface RwxTriplet {
    r: boolean;
    w: boolean;
    x: boolean;
}
/** Split an octal mode into [special, owner, group, other], where `special`
 *  carries the setuid(4)/setgid(2)/sticky(1) bits. */
export declare function octalToTriplets(octal: string): [number, number, number, number];
/** Recombine the special + rwx triplets into an octal string. The special digit
 *  is only prefixed when non-zero, so ordinary modes stay 3 digits. */
export declare function tripletsToOctal(special: number, owner: number, group: number, other: number): string;
export declare function bitsToRwx(bits: number): RwxTriplet;
export declare function rwxToBits(t: RwxTriplet): number;
export declare function parseModeInput(input: string): string | null;
//# sourceMappingURL=filePermissions.d.ts.map