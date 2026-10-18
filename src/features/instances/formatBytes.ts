const MEBIBYTE = 1024 * 1024;

/** Downloaded bytes as megabytes with one decimal (the unit label goes next to it). */
export function formatBytes(bytes: number): string {
  return (Math.max(0, bytes) / MEBIBYTE).toFixed(1);
}
